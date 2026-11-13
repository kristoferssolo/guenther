use crate::{
    ffmpeg::Ffmpeg,
    llm::{
        error::{LlmError, Result},
        openrouter::ContentPart,
    },
    utils::MediaKind,
};
use futures::future::try_join_all;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use tokio::fs;
use tracing::warn;

/// Upper bound on images, frames, and videos sent with one caption request.
const MAX_VISUAL_PARTS: u32 = 4;
const FRAMES_PER_VIDEO: u32 = 3;
/// 20 MiB. Larger videos fall back to a single frame, since the base64 body
/// grows by a third.
const VIDEO_INPUT_MAX_BYTES: u64 = 20_971_520;
/// Caps frame width; `-2` keeps the aspect ratio with an even height.
const FRAME_SCALE_FILTER: &str = "scale='min(768,iw)':-2";
const FRAME_MIME_TYPE: &str = "image/jpeg";
const DEFAULT_VIDEO_MIME_TYPE: &str = "video/mp4";

/// A downloaded image or video on local disk.
#[derive(Debug, Clone)]
pub struct LocalMedia {
    pub path: PathBuf,
    pub kind: MediaKind,
}

/// A model input part with a short label telling the model what it is.
#[derive(Debug)]
pub struct LabeledPart {
    pub label: String,
    pub part: ContentPart,
}

/// How one media item is presented to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualPlan {
    Image,
    Video,
    Frames(u32),
}

/// Spend the visual budget on leading media items in order.
///
/// The result lines up with the first `len()` items; later items are not
/// sent. Videos cost [`FRAMES_PER_VIDEO`] parts unless sent as video input.
pub fn plan_visuals(
    kinds: impl IntoIterator<Item = MediaKind>,
    video_input: bool,
) -> Vec<VisualPlan> {
    let mut remaining = MAX_VISUAL_PARTS;
    let mut plans = Vec::new();
    for kind in kinds {
        if remaining == 0 {
            break;
        }
        let (plan, cost) = match kind {
            MediaKind::Image => (VisualPlan::Image, 1),
            MediaKind::Video if video_input => (VisualPlan::Video, 1),
            MediaKind::Video => {
                let frames = FRAMES_PER_VIDEO.min(remaining);
                (VisualPlan::Frames(frames), frames)
            }
        };
        remaining = remaining.saturating_sub(cost);
        plans.push(plan);
    }
    plans
}

/// Evenly spaced timestamps at the centers of `count` equal slices, e.g. 1/6,
/// 1/2, and 5/6 of the clip for three frames. Without a usable duration only
/// the first frame is taken.
pub fn frame_timestamps(duration_secs: Option<f64>, count: u32) -> Vec<f64> {
    let Some(duration) = duration_secs.filter(|secs| secs.is_finite() && *secs > 0.0) else {
        return vec![0.0];
    };
    (0..count)
        .map(|index| (f64::from(index) + 0.5) / f64::from(count) * duration)
        .collect()
}

/// Turns local media into model input parts using ffmpeg and ffprobe.
#[derive(Debug, Clone)]
pub struct MediaTools {
    ffmpeg: Ffmpeg,
    video_input: bool,
}

impl MediaTools {
    pub const fn new(ffmpeg: Ffmpeg, video_input: bool) -> Self {
        Self {
            ffmpeg,
            video_input,
        }
    }

    pub fn plan(&self, kinds: impl IntoIterator<Item = MediaKind>) -> Vec<VisualPlan> {
        plan_visuals(kinds, self.video_input)
    }

    /// Build labeled parts for the planned items, skipping items that fail.
    ///
    /// Extracted frames are written to `work_dir`.
    pub async fn prepare(&self, media: &[LocalMedia], work_dir: &Path) -> Vec<LabeledPart> {
        let plans = self.plan(media.iter().map(|item| item.kind));
        let mut parts = Vec::new();
        for ((number, item), plan) in (1_u32..).zip(media).zip(plans) {
            match self.prepare_item(number, item, plan, work_dir).await {
                Ok(item_parts) => parts.extend(item_parts),
                Err(err) => warn!(%err, item = number, "Skipping media item for the LLM caption"),
            }
        }
        parts
    }

    async fn prepare_item(
        &self,
        number: u32,
        item: &LocalMedia,
        plan: VisualPlan,
        work_dir: &Path,
    ) -> Result<Vec<LabeledPart>> {
        match plan {
            VisualPlan::Image => {
                let bytes = fs::read(&item.path).await?;
                let mime_type = detect_mime(&bytes, "image/")?;
                Ok(vec![LabeledPart {
                    label: format!("Item {number}, image:"),
                    part: ContentPart::image(mime_type, &bytes),
                }])
            }
            VisualPlan::Video => {
                if fs::metadata(&item.path).await?.len() > VIDEO_INPUT_MAX_BYTES {
                    return self.frames(number, &item.path, 1, work_dir).await;
                }
                let bytes = fs::read(&item.path).await?;
                let mime_type = detect_mime(&bytes, "video/").unwrap_or(DEFAULT_VIDEO_MIME_TYPE);
                Ok(vec![LabeledPart {
                    label: format!("Item {number}, video:"),
                    part: ContentPart::video(mime_type, &bytes),
                }])
            }
            VisualPlan::Frames(count) => self.frames(number, &item.path, count, work_dir).await,
        }
    }

    async fn frames(
        &self,
        number: u32,
        input: &Path,
        count: u32,
        work_dir: &Path,
    ) -> Result<Vec<LabeledPart>> {
        let timestamps = frame_timestamps(self.ffmpeg.probe_duration(input).await, count);
        let total = timestamps.len();
        let extractions = (1_usize..).zip(timestamps).map(|(frame, timestamp)| {
            let output_path = work_dir.join(format!("item-{number}-frame-{frame}.jpg"));
            async move {
                self.extract_frame(input, timestamp, &output_path).await?;
                let bytes = fs::read(&output_path).await?;
                Ok::<_, LlmError>(LabeledPart {
                    label: format!("Item {number}, video frame {frame} of {total}:"),
                    part: ContentPart::image(FRAME_MIME_TYPE, &bytes),
                })
            }
        });
        try_join_all(extractions).await
    }

    async fn extract_frame(&self, input: &Path, timestamp: f64, output_path: &Path) -> Result<()> {
        let timestamp = format!("{timestamp:.3}");
        self.ffmpeg
            .run([
                OsStr::new("-ss"),
                OsStr::new(&timestamp),
                OsStr::new("-i"),
                input.as_os_str(),
                OsStr::new("-frames:v"),
                OsStr::new("1"),
                OsStr::new("-vf"),
                OsStr::new(FRAME_SCALE_FILTER),
                OsStr::new("-q:v"),
                OsStr::new("4"),
                output_path.as_os_str(),
            ])
            .await?;
        Ok(())
    }
}

fn detect_mime(bytes: &[u8], prefix: &str) -> Result<&'static str> {
    let mime_type = infer::get(bytes).map_or("application/octet-stream", |kind| kind.mime_type());
    if mime_type.starts_with(prefix) {
        return Ok(mime_type);
    }
    Err(LlmError::UnsupportedMedia(mime_type.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FfmpegConfig;
    use claims::{assert_ok, assert_ok_eq};
    use rstest::rstest;
    use tempfile::tempdir;
    use tokio::process::Command;

    use MediaKind::{Image, Video};
    use VisualPlan::Frames;

    #[rstest]
    #[case::single_video(&[Video], false, &[Frames(3)])]
    #[case::video_then_images(&[Video, Image, Image], false, &[Frames(3), VisualPlan::Image])]
    #[case::second_video_gets_leftover_frame(&[Video, Video], false, &[Frames(3), Frames(1)])]
    #[case::gallery_capped(&[Image; 6], false, &[VisualPlan::Image; 4])]
    #[case::video_input(&[Video, Video, Image], true, &[VisualPlan::Video, VisualPlan::Video, VisualPlan::Image])]
    #[case::empty(&[], false, &[])]
    fn visual_budget_covers_leading_items(
        #[case] kinds: &[MediaKind],
        #[case] video_input: bool,
        #[case] expected: &[VisualPlan],
    ) {
        assert_eq!(plan_visuals(kinds.iter().copied(), video_input), expected);
    }

    #[rstest]
    #[case::three_frames(Some(6.0), 3, &[1.0, 3.0, 5.0])]
    #[case::one_frame(Some(10.0), 1, &[5.0])]
    #[case::unknown_duration(None, 3, &[0.0])]
    #[case::zero_duration(Some(0.0), 3, &[0.0])]
    #[case::nan_duration(Some(f64::NAN), 3, &[0.0])]
    fn frames_are_centered_in_equal_slices(
        #[case] duration: Option<f64>,
        #[case] count: u32,
        #[case] expected: &[f64],
    ) {
        assert_eq!(frame_timestamps(duration, count), expected);
    }

    #[test]
    fn detect_mime_rejects_wrong_media_class() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert_ok_eq!(detect_mime(&png, "image/"), "image/png");
        assert!(detect_mime(&png, "video/").is_err());
        assert!(detect_mime(b"plain text", "image/").is_err());
    }

    /// Needs ffmpeg and ffprobe on `PATH`.
    #[tokio::test]
    #[ignore = "requires ffmpeg and ffprobe"]
    async fn extracts_three_frames_from_a_video() {
        let dir = assert_ok!(tempdir());
        let video = dir.path().join("clip.mp4");
        let status = assert_ok!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=duration=3:size=1280x720:rate=10"
                ])
                .arg(&video)
                .status()
                .await
        );
        assert!(status.success());

        let tools = MediaTools::new(Ffmpeg::new(&FfmpegConfig::default()), false);
        let media = [LocalMedia {
            path: video,
            kind: Video,
        }];
        let parts = tools.prepare(&media, dir.path()).await;

        let labels = parts
            .iter()
            .map(|part| part.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "Item 1, video frame 1 of 3:",
                "Item 1, video frame 2 of 3:",
                "Item 1, video frame 3 of 3:",
            ]
        );
        assert!(parts.iter().all(|part| matches!(
            &part.part,
            ContentPart::ImageUrl { image_url } if image_url.url.starts_with("data:image/jpeg;base64,")
        )));
    }
}
