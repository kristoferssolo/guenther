use std::{ffi::OsStr, path::Path};
use strum::{Display, EnumString, IntoStaticStr};
use tokio::{fs::File, io::AsyncReadExt};
use tracing::warn;

pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mov", "mkv", "avi", "m4v", "3gp"];
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];

/// Media type supported by the Telegram handler.
///
/// The lowercase names are stored in `media_cache.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum MediaKind {
    Video,
    Image,
}

fn ext_matches(ext: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

fn detect_from_extension(path: &Path) -> Option<MediaKind> {
    let ext = path.extension().and_then(OsStr::to_str)?;
    if ext_matches(ext, VIDEO_EXTENSIONS) {
        return Some(MediaKind::Video);
    }
    if ext_matches(ext, IMAGE_EXTENSIONS) {
        return Some(MediaKind::Image);
    }
    None
}

fn detect_from_mime(mime_type: &str) -> Option<MediaKind> {
    match mime_type.split('/').next() {
        Some("video") => Some(MediaKind::Video),
        Some("image") => Some(MediaKind::Image),
        _ => None,
    }
}

/// Detect media kind without blocking on file I/O, or `None` when the file is
/// neither a video nor an image.
pub async fn detect_media_kind(path: &Path) -> Option<MediaKind> {
    if let Some(kind) = detect_from_extension(path) {
        return Some(kind);
    }

    let Ok(mut file) = File::open(path).await else {
        warn!(path = ?path.display(), "Failed to open file for media detection");
        return None;
    };

    let mut buffer = vec![0u8; 8192];
    if let Ok(n) = file.read(&mut buffer).await
        && n > 0
    {
        buffer.truncate(n);
        if let Some(k) = infer::get(&buffer) {
            return detect_from_mime(k.mime_type());
        }
    }

    None
}

/// Truncate text by Unicode scalar values and append `...` when needed.
pub fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let truncated = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok_eq, assert_some_eq};
    use rstest::rstest;

    #[rstest]
    #[case("video/mp4", Some(MediaKind::Video))]
    #[case("image/png", Some(MediaKind::Image))]
    #[case("text/plain", None)]
    fn detect_media_kind_by_mime(#[case] mime_type: &str, #[case] expected: Option<MediaKind>) {
        assert_eq!(detect_from_mime(mime_type), expected);
    }

    #[rstest]
    #[case(MediaKind::Video, "video")]
    #[case(MediaKind::Image, "image")]
    fn media_kind_round_trips_stored_names(#[case] kind: MediaKind, #[case] name: &str) {
        assert_eq!(<&str>::from(kind), name);
        assert_ok_eq!(name.parse::<MediaKind>(), kind);
    }

    #[test]
    fn detect_media_kind_by_extension() {
        assert_some_eq!(
            detect_from_extension(Path::new("video.mp4")),
            MediaKind::Video
        );
        assert_some_eq!(
            detect_from_extension(Path::new("image.jpg")),
            MediaKind::Image
        );
        assert_none!(detect_from_extension(Path::new("unknown.txt")));
    }

    #[test]
    fn media_kind_case_insensitive() {
        assert_some_eq!(
            detect_from_extension(Path::new("VIDEO.MP4")),
            MediaKind::Video
        );
        assert_some_eq!(
            detect_from_extension(Path::new("IMAGE.JPG")),
            MediaKind::Image
        );
    }

    #[test]
    fn truncates_without_splitting_utf8_text() {
        assert_eq!(truncate_with_ellipsis("éclair", 5), "éc...");
        assert_eq!(truncate_with_ellipsis("abcdef", 3), "...");
        assert_eq!(truncate_with_ellipsis("abcdef", 2), "..");
    }
}
