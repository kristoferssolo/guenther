use crate::error::{Error, Result};
use rand::{rng, seq::IndexedRandom};
use std::{fmt::Display, path::Path, sync::OnceLock};
use tokio::fs::read_to_string;

static GLOBAL_COMMENTS: OnceLock<Comments> = OnceLock::new();

pub const TELEGRAM_CAPTION_LIMIT: usize = 4096;
const FALLBACK_COMMENTS: &[&str] = &[
    "Oh come on, that's brilliant – and slightly chaotic, like always.",
    "That is a proper bit of craftsmanship – then someone presses the red button.",
    "Nice shot – looks good on the trailer, not so good on the gearbox.",
    "Here you go. Judge for yourself.",
];
const FAILURE_COMMENTS: &[&str] = &[
    "Failed to fetch the media, you foking donkey.",
    "The link is there, the media is not. What a foking mess.",
    "I asked for media and got nothing. We look like a bunch of wankers.",
    "The download foksmashed itself before it even started.",
    "No video, no photo, no explanation. Fantastic.",
    "This link has the pace of a foking milk float.",
    "The server says no. Tell the server to fok off.",
    "We tried everything. It is still completely foked.",
    "The media disappeared faster than our race pace.",
    "Another link, another foking disaster.",
    "I cannot send what I cannot download. Even I cannot fix this shit.",
    "The downloader brought back half a file. Useless.",
    "Nothing came out of this link except disappointment.",
    "The link promised media and delivered fok all.",
    "Call Gene. Tell him the download budget is gone.",
];

#[derive(Debug)]
pub struct Comments {
    lines: Vec<String>,
}

impl Comments {
    /// Create a small dummy/default Comments instance (useful for tests or fallback).
    #[must_use]
    pub fn dummy() -> Self {
        let lines = FALLBACK_COMMENTS
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Self { lines }
    }

    /// Load comments from a plaintext file asynchronously.
    ///
    /// # Errors
    ///
    /// - Returns `Error::Io` if reading the file fails.
    /// - Returns `Error::Other` if the file contains no usable lines.
    pub async fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = read_to_string(path).await?;

        let lines = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        if lines.is_empty() {
            return Err(Error::other("comments file contains no usable lines"));
        }

        Ok(Self { lines })
    }

    /// Pick a random comment. Falls back to a default if the list is empty.
    #[must_use]
    pub fn pick(&self) -> &str {
        let mut rng = rng();
        self.lines
            .choose(&mut rng)
            .map_or(FALLBACK_COMMENTS[0], AsRef::as_ref)
    }

    /// Build a caption by picking a random comment and truncating if necessary.
    #[must_use]
    pub fn build_caption(&self) -> String {
        let mut caption = self.pick().to_string();

        // Truncate if too long for Telegram
        if caption.chars().count() > TELEGRAM_CAPTION_LIMIT {
            let truncated = caption
                .chars()
                .take(TELEGRAM_CAPTION_LIMIT.saturating_sub(3))
                .collect::<String>();
            caption = format!("{truncated}...");
        }
        caption
    }

    /// Get a reference to the underlying lines for debugging or testing.
    #[cfg(test)]
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Initialize the global comments (call once at startup).
    ///
    /// # Errors
    ///
    /// Returns `Error::Other` when the global is already initialized.
    pub fn init(self) -> Result<()> {
        GLOBAL_COMMENTS
            .set(self)
            .map_err(|_| Error::other("comments already initialized"))
    }
}

/// Get global comments (initialized by `Comments::init(self)`).
///
/// # Panics
///
/// Panics if comments have not been initialized.
#[inline]
#[must_use]
pub fn global_comments() -> &'static Comments {
    GLOBAL_COMMENTS.get().expect("comments not initialized")
}

/// Pick a random built-in response for a failed media download.
#[must_use]
pub fn failure_comment() -> &'static str {
    let mut rng = rng();
    FAILURE_COMMENTS
        .choose(&mut rng)
        .copied()
        .unwrap_or("Failed to fetch media.")
}

impl Display for Comments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.build_caption())
    }
}

impl From<Comments> for String {
    fn from(value: Comments) -> Self {
        value.to_string()
    }
}

impl From<&Comments> for String {
    fn from(value: &Comments) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dummy_comments() {
        let comments = Comments::dummy();
        assert_eq!(comments.lines.len(), FALLBACK_COMMENTS.len());
        assert!(!comments.lines.is_empty());
    }

    #[test]
    fn build_caption_truncation() {
        let long_comment = "A".repeat(TELEGRAM_CAPTION_LIMIT + 10);
        let comments = Comments {
            lines: vec![long_comment],
        };

        let caption = comments.build_caption();
        assert_eq!(caption.chars().count(), TELEGRAM_CAPTION_LIMIT);
        assert!(caption.ends_with("..."));
    }

    #[test]
    fn pick_fallback() {
        let empty_comment = Comments { lines: Vec::new() };
        assert_eq!(empty_comment.pick(), FALLBACK_COMMENTS[0]);
    }

    #[test]
    fn failure_comments_are_valid_telegram_messages() {
        assert!(!FAILURE_COMMENTS.is_empty());
        assert!(FAILURE_COMMENTS.iter().all(|comment| {
            !comment.trim().is_empty() && comment.chars().count() <= TELEGRAM_CAPTION_LIMIT
        }));
    }

    #[test]
    fn picks_failure_comment_from_pool() {
        assert!(FAILURE_COMMENTS.contains(&failure_comment()));
    }
}
