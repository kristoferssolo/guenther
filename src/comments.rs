use crate::utils::truncate_with_ellipsis;
use rand::{rng, seq::IndexedRandom};
use std::{io, path::Path};
use thiserror::Error;
use tokio::fs::read_to_string;

#[derive(Debug, Error)]
pub enum CommentsError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Comments file contains no usable lines")]
    Empty,
}

pub type Result<T> = std::result::Result<T, CommentsError>;

/// Telegram caps media captions at 1024 characters, below the 4096 allowed for
/// text messages.
pub const TELEGRAM_CAPTION_LIMIT: usize = 1024;
const DEFAULT_COMMENT: &str = "Here you go. Judge for yourself.";
const FALLBACK_COMMENTS: &[&str] = &[
    "Oh come on, that's brilliant – and slightly chaotic, like always.",
    "That is a proper bit of craftsmanship – then someone presses the red button.",
    "Nice shot – looks good on the trailer, not so good on the gearbox.",
    DEFAULT_COMMENT,
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
    /// Load comments from a plaintext file asynchronously.
    ///
    /// # Errors
    ///
    /// - Returns [`CommentsError::Io`] if reading the file fails.
    /// - Returns [`CommentsError::Empty`] if the file contains no usable lines.
    pub async fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = read_to_string(path).await?;

        let lines = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        if lines.is_empty() {
            return Err(CommentsError::Empty);
        }

        Ok(Self { lines })
    }

    /// Pick a random comment. Falls back to a default if the list is empty.
    pub fn pick(&self) -> &str {
        let mut rng = rng();
        self.lines
            .choose(&mut rng)
            .map_or(DEFAULT_COMMENT, AsRef::as_ref)
    }

    /// Build a caption by picking a random comment and truncating if necessary.
    pub fn build_caption(&self) -> String {
        truncate_with_ellipsis(self.pick(), TELEGRAM_CAPTION_LIMIT)
    }
}

impl Default for Comments {
    fn default() -> Self {
        Self {
            lines: FALLBACK_COMMENTS.iter().map(ToString::to_string).collect(),
        }
    }
}

/// Pick a random built-in response for a failed media download.
pub fn failure_comment() -> &'static str {
    let mut rng = rng();
    FAILURE_COMMENTS
        .choose(&mut rng)
        .copied()
        .unwrap_or("Failed to fetch media.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use std::{assert_matches, fs};
    use tempfile::tempdir;

    async fn load(contents: &str) -> Result<Comments> {
        let dir = assert_ok!(tempdir());
        let path = dir.path().join("lines.txt");
        assert_ok!(fs::write(&path, contents));
        Comments::load_from_file(&path).await
    }

    #[tokio::test]
    async fn loads_trimmed_lines_in_order_skipping_blank_and_comment_lines() {
        let comments =
            assert_ok!(load("  first  \n\n# note\n\tsecond\n   \n  # indented\nthird\n").await);
        assert_eq!(comments.lines, ["first", "second", "third"]);
    }

    #[tokio::test]
    async fn rejects_file_without_usable_lines() {
        assert_matches!(
            load("\n   \n# note\n  # indented\n").await,
            Err(CommentsError::Empty)
        );
    }

    #[tokio::test]
    async fn reports_missing_file_as_io_error() {
        let dir = assert_ok!(tempdir());
        assert_matches!(
            Comments::load_from_file(dir.path().join("missing.txt")).await,
            Err(CommentsError::Io(_))
        );
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
        assert_eq!(empty_comment.pick(), DEFAULT_COMMENT);
    }

    #[test]
    fn failure_comments_are_valid_telegram_messages() {
        assert_ne!(FAILURE_COMMENTS, Vec::<&str>::new());
        assert!(FAILURE_COMMENTS.iter().all(|comment| {
            !comment.trim().is_empty() && comment.chars().count() <= TELEGRAM_CAPTION_LIMIT
        }));
    }
}
