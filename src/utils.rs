use crate::{
    comments::global_comments,
    error::{Error, Result},
};
use capitalize::Capitalize;
use std::{
    ffi::OsStr,
    fmt::Display,
    path::{Path, PathBuf},
};
use teloxide::{prelude::*, types::InputFile};
use tokio::{fs::File, io::AsyncReadExt};
use tracing::{error, info, warn};

pub const VIDEO_EXSTENSIONS: &[&str] = &["mp4", "webm", "mov", "mkv", "avi", "m4v", "3gp"];
pub const IMAGE_EXSTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];

/// Simple media kind enum shared by handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Image,
    Unknown,
}

impl MediaKind {
    #[must_use]
    #[inline]
    pub const fn to_str(&self) -> &str {
        match self {
            Self::Video => "video",
            Self::Image => "image",
            Self::Unknown => "unknown",
        }
    }
}

/// Check if extension matches any in the given list (case-insensitive).
#[inline]
fn ext_matches(ext: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// Detect media kind from file extension.
fn detect_from_extension(path: &Path) -> Option<MediaKind> {
    let ext = path.extension().and_then(OsStr::to_str)?;
    if ext_matches(ext, VIDEO_EXSTENSIONS) {
        return Some(MediaKind::Video);
    }
    if ext_matches(ext, IMAGE_EXSTENSIONS) {
        return Some(MediaKind::Image);
    }
    None
}

/// Detect media kind from MIME type string.
fn detect_from_mime(mime_type: &str) -> MediaKind {
    match mime_type.split('/').next() {
        Some("video") => MediaKind::Video,
        Some("image") => MediaKind::Image,
        _ => MediaKind::Unknown,
    }
}

/// Detect media kind first by extension, then by content/magic (sync).
#[must_use]
pub fn detect_media_kind(path: &Path) -> MediaKind {
    if let Some(kind) = detect_from_extension(path) {
        return kind;
    }

    // Fallback to MIME type detection
    if let Ok(Some(kind)) = infer::get_from_path(path) {
        return detect_from_mime(kind.mime_type());
    }

    MediaKind::Unknown
}

/// Async/non-blocking detection: check extension first, otherwise read a small
/// sample asynchronously and run `infer::get` on the buffer.
pub async fn detect_media_kind_async(path: &Path) -> MediaKind {
    if let Some(kind) = detect_from_extension(path) {
        return kind;
    }

    // Read a small prefix (8 KiB) asynchronously and probe
    let Ok(mut file) = File::open(path).await else {
        warn!(path = ?path.display(), "Failed to open file for media detection");
        return MediaKind::Unknown;
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

    MediaKind::Unknown
}

/// Given a path, send it to chat as photo or video depending on detected kind.
///
/// # Errors
///
/// Returns an `Error::UnknownMediaKind` if sending fails or the media kind is unknown.
pub async fn send_media_from_path(
    bot: &Bot,
    chat_id: ChatId,
    path: PathBuf,
    kind: MediaKind,
) -> Result<()> {
    let caption = global_comments().build_caption();
    let input = InputFile::file(path);

    macro_rules! send_msg {
        ($request_expr:expr) => {{
            let mut request = $request_expr;
            request = request.caption(caption);
            match request.await {
                Ok(message) => info!(message_id = message.id.to_string(), "{} sent", kind),
                Err(e) => {
                    error!("Failed to send {}: {e}", kind.to_str());
                    return Err(Error::Teloxide(e));
                }
            }
        }};
    }

    match kind {
        MediaKind::Video => send_msg!(bot.send_video(chat_id, input)),
        MediaKind::Image => send_msg!(bot.send_photo(chat_id, input)),
        MediaKind::Unknown => {
            bot.send_message(chat_id, "No supported media found")
                .await?;
            error!("No supported media found");
            return Err(Error::UnknownMediaKind);
        }
    }

    Ok(())
}

impl AsRef<str> for MediaKind {
    fn as_ref(&self) -> &str {
        self.to_str()
    }
}

impl Display for MediaKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.capitalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_media_kind_by_extension() {
        assert_eq!(detect_media_kind(Path::new("video.mp4")), MediaKind::Video);
        assert_eq!(detect_media_kind(Path::new("image.jpg")), MediaKind::Image);
        assert_eq!(
            detect_media_kind(Path::new("unknown.txt")),
            MediaKind::Unknown
        );
    }

    #[test]
    fn media_kind_case_insensitive() {
        assert_eq!(detect_media_kind(Path::new("VIDEO.MP4")), MediaKind::Video);
        assert_eq!(detect_media_kind(Path::new("IMAGE.JPG")), MediaKind::Image);
    }
}
