use crate::{
    config::TelegramConfig,
    ffmpeg::{Ffmpeg, FfmpegError},
    utils::MediaKind,
};
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    fs,
    sync::{AcquireError, Semaphore},
    time::timeout,
};
use tracing::{info, warn};

/// Longest video worth transcoding; longer ones get the too-large reply
/// instead of tying up the host.
const MAX_TRANSCODE_DURATION: Duration = Duration::from_mins(10);
const TRANSCODE_TIMEOUT: Duration = Duration::from_mins(5);
/// Share of the limit a transcode aims for, leaving room for single-pass
/// overshoot and container overhead.
const TARGET_PERCENT: u64 = 90;
const AUDIO_KBPS: u32 = 96;
/// Below this a transcode is not worth watching.
const MIN_VIDEO_KBPS: u32 = 200;
const MAX_IMAGE_SIDE: u32 = 2560;

/// Upload limits of a Bot API server, in bytes.
///
/// Telegram states them in megabytes; decimal megabytes stay safely under
/// what the servers enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadLimits {
    pub file: u64,
    pub photo: u64,
}

impl UploadLimits {
    pub const CLOUD: Self = Self {
        file: 50_000_000,
        photo: 10_000_000,
    };
    /// A `telegram-bot-api` server in `--local` mode. Photos keep the cloud
    /// limit.
    pub const LOCAL: Self = Self {
        file: 2_000_000_000,
        photo: 10_000_000,
    };

    /// Limits for the configured Bot API server.
    pub const fn for_telegram(config: &TelegramConfig) -> Self {
        if config.is_local() {
            Self::LOCAL
        } else {
            Self::CLOUD
        }
    }

    const fn for_kind(self, kind: MediaKind) -> u64 {
        match kind {
            MediaKind::Video => self.file,
            MediaKind::Image => self.photo,
        }
    }

    const fn fits(self, kind: MediaKind, size: u64) -> bool {
        size <= self.for_kind(kind)
    }
}

/// Video bitrate in kbit/s that fits `duration` into `limit_bytes` next to the
/// audio track, or `None` when the video is too long to transcode or the
/// bitrate would be unwatchable.
fn video_bitrate_kbps(limit_bytes: u64, duration: Duration) -> Option<u32> {
    if duration > MAX_TRANSCODE_DURATION {
        return None;
    }
    let budget_bits = u128::from(limit_bytes)
        .checked_mul(8)?
        .checked_mul(u128::from(TARGET_PERCENT))?
        .checked_div(100)?;
    // Bits per millisecond are kbit/s.
    let total_kbps = budget_bits.checked_div(duration.as_millis())?;
    let video_kbps = total_kbps.checked_sub(u128::from(AUDIO_KBPS))?;
    let video_kbps = u32::try_from(video_kbps).unwrap_or(u32::MAX);
    (video_kbps >= MIN_VIDEO_KBPS).then_some(video_kbps)
}

/// Longest output side for a video at `video_kbps`, so low bitrates spend
/// their bits on fewer pixels.
const fn max_video_side(video_kbps: u32) -> u32 {
    match video_kbps {
        4000.. => 1920,
        1500.. => 1280,
        _ => 854,
    }
}

/// Scale filter that shrinks either side to at most `side`, keeping the aspect
/// ratio and leaving smaller inputs alone.
fn scale_filter(side: u32) -> String {
    format!(
        "scale=w='min({side},iw)':h='min({side},ih)':force_original_aspect_ratio=decrease:force_divisible_by=2"
    )
}

fn video_args(input: &Path, output: &Path, video_kbps: u32) -> Vec<OsString> {
    let bitrate = format!("{video_kbps}k");
    let bufsize = format!("{}k", video_kbps.saturating_mul(2));
    let audio_bitrate = format!("{AUDIO_KBPS}k");
    [
        "-i".into(),
        input.into(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a:0?".into(),
        "-vf".into(),
        scale_filter(max_video_side(video_kbps)).into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-b:v".into(),
        bitrate.clone().into(),
        "-maxrate".into(),
        bitrate.into(),
        "-bufsize".into(),
        bufsize.into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        audio_bitrate.into(),
        "-movflags".into(),
        "+faststart".into(),
        output.into(),
    ]
    .into()
}

fn image_args(input: &Path, output: &Path) -> Vec<OsString> {
    [
        "-i".into(),
        input.into(),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        scale_filter(MAX_IMAGE_SIDE).into(),
        "-q:v".into(),
        "3".into(),
        output.into(),
    ]
    .into()
}

#[derive(Debug, Error)]
enum FitError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error(transparent)]
    Ffmpeg(#[from] FfmpegError),

    #[error("Transcode did not finish within {0:?}")]
    Timeout(Duration),

    #[error("Transcode queue closed: {0}")]
    Queue(#[from] AcquireError),
}

/// Downloaded media brought under the upload limits.
#[derive(Debug, Default)]
pub(super) struct FittedMedia {
    pub items: Vec<(PathBuf, MediaKind)>,
    /// Items that could not be brought under the limit.
    pub dropped: usize,
}

/// Shrinks downloaded media that exceeds the Bot API upload limits.
#[derive(Debug, Clone)]
pub struct MediaFitter {
    ffmpeg: Ffmpeg,
    limits: UploadLimits,
    /// Shared by all clones so concurrent links run one transcode at a time.
    transcodes: Arc<Semaphore>,
}

impl MediaFitter {
    pub fn new(ffmpeg: Ffmpeg, limits: UploadLimits) -> Self {
        Self {
            ffmpeg,
            limits,
            transcodes: Arc::new(Semaphore::new(1)),
        }
    }

    /// Keep items within the limits, re-encode oversized ones into
    /// `work_dir`, and drop those that still cannot fit.
    pub(super) async fn fit(
        &self,
        items: Vec<(PathBuf, MediaKind)>,
        work_dir: &Path,
    ) -> FittedMedia {
        let mut fitted = FittedMedia::default();
        for (number, (path, kind)) in (1_usize..).zip(items) {
            match self.fit_item(number, &path, kind, work_dir).await {
                Ok(Some(path)) => fitted.items.push((path, kind)),
                Ok(None) => fitted.dropped = fitted.dropped.saturating_add(1),
                Err(err) => {
                    warn!(%err, item = number, %kind, "Failed to shrink oversized media");
                    fitted.dropped = fitted.dropped.saturating_add(1);
                }
            }
        }
        fitted
    }

    async fn fit_item(
        &self,
        number: usize,
        input: &Path,
        kind: MediaKind,
        work_dir: &Path,
    ) -> Result<Option<PathBuf>, FitError> {
        let size = fs::metadata(input).await?.len();
        if self.limits.fits(kind, size) {
            return Ok(Some(input.to_owned()));
        }

        let limit = self.limits.for_kind(kind);
        let (output, args) = match kind {
            MediaKind::Video => {
                let duration = self
                    .ffmpeg
                    .probe_duration(input)
                    .await
                    .and_then(|secs| Duration::try_from_secs_f64(secs).ok());
                let Some(video_kbps) =
                    duration.and_then(|duration| video_bitrate_kbps(limit, duration))
                else {
                    info!(
                        item = number,
                        size,
                        limit,
                        ?duration,
                        "Video is too large to transcode"
                    );
                    return Ok(None);
                };
                let output = work_dir.join(format!("fit-{number}.mp4"));
                let args = video_args(input, &output, video_kbps);
                (output, args)
            }
            MediaKind::Image => {
                let output = work_dir.join(format!("fit-{number}.jpg"));
                let args = image_args(input, &output);
                (output, args)
            }
        };

        info!(item = number, %kind, size, limit, "Shrinking media over the upload limit");
        let _permit = self.transcodes.acquire().await?;
        timeout(TRANSCODE_TIMEOUT, self.ffmpeg.run(args))
            .await
            .map_err(|_| FitError::Timeout(TRANSCODE_TIMEOUT))??;

        let shrunk_size = fs::metadata(&output).await?.len();
        if !self.limits.fits(kind, shrunk_size) {
            info!(item = number, %kind, shrunk_size, limit, "Shrunk media is still over the limit");
            return Ok(None);
        }
        info!(item = number, %kind, size, shrunk_size, "Shrunk media under the upload limit");
        Ok(Some(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some, assert_some_eq};
    use rstest::rstest;

    #[rstest]
    #[case::video_at_limit(MediaKind::Video, 50_000_000, true)]
    #[case::video_over_limit(MediaKind::Video, 50_000_001, false)]
    #[case::photo_at_limit(MediaKind::Image, 10_000_000, true)]
    #[case::photo_over_photo_limit(MediaKind::Image, 10_000_001, false)]
    #[case::video_over_photo_limit(MediaKind::Video, 10_000_001, true)]
    fn cloud_size_decision(#[case] kind: MediaKind, #[case] size: u64, #[case] fits: bool) {
        assert_eq!(UploadLimits::CLOUD.fits(kind, size), fits);
    }

    #[test]
    fn local_server_raises_only_the_file_limit() {
        assert!(UploadLimits::LOCAL.fits(MediaKind::Video, 1_500_000_000));
        assert!(!UploadLimits::LOCAL.fits(MediaKind::Image, 10_000_001));
    }

    #[rstest]
    #[case::one_minute(50_000_000, 60, 5_904)]
    #[case::at_duration_ceiling(50_000_000, 600, 504)]
    #[case::local_server(2_000_000_000, 600, 23_904)]
    fn bitrate_fills_the_limit_with_headroom(
        #[case] limit_bytes: u64,
        #[case] duration_secs: u64,
        #[case] expected_kbps: u32,
    ) {
        assert_some_eq!(
            video_bitrate_kbps(limit_bytes, Duration::from_secs(duration_secs)),
            expected_kbps
        );
    }

    #[test]
    fn transcoded_bitrate_fits_the_limit() {
        let duration = Duration::from_secs(137);
        let video_kbps = assert_some!(video_bitrate_kbps(UploadLimits::CLOUD.file, duration));
        let bytes = u64::from(video_kbps)
            .saturating_add(u64::from(AUDIO_KBPS))
            .saturating_mul(1000 / 8)
            .saturating_mul(duration.as_secs());
        assert!(bytes < UploadLimits::CLOUD.file);
    }

    #[rstest]
    #[case::over_duration_ceiling(50_000_000, Duration::from_secs(601))]
    #[case::zero_duration(50_000_000, Duration::ZERO)]
    #[case::sub_millisecond(50_000_000, Duration::from_micros(10))]
    #[case::below_quality_floor(1_000_000, Duration::from_secs(60))]
    fn bitrate_refuses_infeasible_transcodes(#[case] limit_bytes: u64, #[case] duration: Duration) {
        assert_none!(video_bitrate_kbps(limit_bytes, duration));
    }

    #[rstest]
    #[case(504, 854)]
    #[case(1_500, 1280)]
    #[case(3_999, 1280)]
    #[case(5_904, 1920)]
    fn lower_bitrates_get_smaller_frames(#[case] video_kbps: u32, #[case] side: u32) {
        assert_eq!(max_video_side(video_kbps), side);
    }
}
