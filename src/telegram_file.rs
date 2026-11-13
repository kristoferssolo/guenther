//! Fetching files that `getFile` describes.

use crate::config::TelegramConfig;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use teloxide::{Bot, DownloadError, net::Download};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt};

#[derive(Debug, Error)]
pub enum TelegramFileError {
    #[error("Bot API returned local path {path} but no local Bot API server is configured")]
    LocalDisabled { path: PathBuf },

    #[error("Bot API returned local path {path} outside its data directory {root}")]
    OutsideLocalDir { path: PathBuf, root: PathBuf },

    #[error("Failed to resolve {path}: {source}")]
    Resolve {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("Failed to read {path} from the local Bot API server: {source}")]
    Copy {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("I/O error while saving a Telegram file: {0}")]
    Io(#[from] io::Error),

    #[error("Failed to download a Telegram file: {0}")]
    Download(#[from] DownloadError),
}

/// Where the file behind a `getFile` path lives.
#[derive(Debug, PartialEq, Eq)]
enum FileSource<'a> {
    /// A `telegram-bot-api --local` server's own filesystem, which it does not
    /// serve over HTTP.
    Local(&'a Path),
    /// A path the Bot API serves for download.
    Remote(&'a str),
}

impl<'a> FileSource<'a> {
    fn new(file_path: &'a str) -> Self {
        let path = Path::new(file_path);
        if path.is_absolute() {
            Self::Local(path)
        } else {
            Self::Remote(file_path)
        }
    }
}

/// Saves files that `getFile` describes, reading local paths only from the
/// configured `telegram-bot-api --local` data directory.
#[derive(Debug, Clone, Default)]
pub struct TelegramFiles {
    local_dir: Option<Arc<Path>>,
}

impl TelegramFiles {
    /// Allow local reads under `local_dir`; `None` rejects every local path.
    pub fn new(local_dir: Option<PathBuf>) -> Self {
        Self {
            local_dir: local_dir.map(Into::into),
        }
    }

    pub fn from_config(config: &TelegramConfig) -> Self {
        Self::new(config.local_dir.clone())
    }

    /// Save the file at `file_path`, as returned by `getFile`, to
    /// `destination`.
    ///
    /// Absolute paths come from a `--local` server and are copied from its
    /// data directory, which must be mounted at the same path.
    ///
    /// # Errors
    ///
    /// Returns an error if a local path is not allowed, or if the file cannot
    /// be copied, downloaded, or written.
    pub async fn save(
        &self,
        bot: &Bot,
        file_path: &str,
        destination: &Path,
    ) -> Result<(), TelegramFileError> {
        match FileSource::new(file_path) {
            FileSource::Local(path) => {
                let source = self.resolve_local(path).await?;
                fs::copy(&source, destination)
                    .await
                    .map_err(|source| TelegramFileError::Copy {
                        path: path.to_owned(),
                        source,
                    })?;
            }
            FileSource::Remote(path) => {
                let mut file = fs::File::create(destination).await?;
                bot.download_file(path, &mut file).await?;
                file.flush().await?;
            }
        }
        Ok(())
    }

    /// Read the file at `file_path`, as returned by `getFile`, into memory.
    ///
    /// # Errors
    ///
    /// Returns an error if a local path is not allowed, or if the file cannot
    /// be read or downloaded.
    pub async fn read(&self, bot: &Bot, file_path: &str) -> Result<Vec<u8>, TelegramFileError> {
        match FileSource::new(file_path) {
            FileSource::Local(path) => {
                let source = self.resolve_local(path).await?;
                fs::read(&source)
                    .await
                    .map_err(|source| TelegramFileError::Copy {
                        path: path.to_owned(),
                        source,
                    })
            }
            FileSource::Remote(path) => {
                let mut bytes = Vec::new();
                bot.download_file(path, &mut bytes).await?;
                Ok(bytes)
            }
        }
    }

    /// Resolve `path` with symlinks and `..` followed, and require it inside
    /// the data directory, so a hostile API server cannot point at other files.
    async fn resolve_local(&self, path: &Path) -> Result<PathBuf, TelegramFileError> {
        let Some(local_dir) = &self.local_dir else {
            return Err(TelegramFileError::LocalDisabled {
                path: path.to_owned(),
            });
        };
        let root = canonicalize(local_dir).await?;
        let resolved = canonicalize(path).await?;
        if !resolved.starts_with(&root) {
            return Err(TelegramFileError::OutsideLocalDir {
                path: path.to_owned(),
                root,
            });
        }
        Ok(resolved)
    }
}

async fn canonicalize(path: &Path) -> Result<PathBuf, TelegramFileError> {
    fs::canonicalize(path)
        .await
        .map_err(|source| TelegramFileError::Resolve {
            path: path.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_matches, assert_ok};
    use rstest::rstest;
    use tempfile::{TempDir, tempdir};

    #[rstest]
    #[case::local_server(
        "/var/lib/telegram-bot-api/123:abc/videos/file_0.mp4",
        FileSource::Local(Path::new("/var/lib/telegram-bot-api/123:abc/videos/file_0.mp4"))
    )]
    #[case::cloud("videos/file_0.mp4", FileSource::Remote("videos/file_0.mp4"))]
    fn absolute_paths_are_read_from_the_local_server(
        #[case] file_path: &str,
        #[case] expected: FileSource<'_>,
    ) {
        assert_eq!(FileSource::new(file_path), expected);
    }

    /// A data directory with `bot/file.mp4` inside and `secret` beside it.
    struct Fixture {
        dir: TempDir,
        files: TelegramFiles,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = assert_ok!(tempdir());
            let root = dir.path().join("data");
            assert_ok!(std::fs::create_dir_all(root.join("bot")));
            assert_ok!(std::fs::write(root.join("bot/file.mp4"), "video"));
            assert_ok!(std::fs::write(dir.path().join("secret"), "secret"));
            Self {
                files: TelegramFiles::new(Some(root)),
                dir,
            }
        }

        fn root(&self) -> PathBuf {
            self.dir.path().join("data")
        }

        async fn save(&self, file_path: &Path) -> Result<String, TelegramFileError> {
            let destination = self.dir.path().join("saved");
            let file_path = file_path.to_str().unwrap_or_default();
            self.files
                .save(&Bot::new("0:test"), file_path, &destination)
                .await?;
            Ok(fs::read_to_string(destination).await?)
        }
    }

    #[tokio::test]
    async fn file_inside_the_data_dir_is_copied() {
        let fixture = Fixture::new();
        let saved = fixture.save(&fixture.root().join("bot/file.mp4")).await;
        assert_eq!(assert_ok!(saved), "video");
    }

    #[tokio::test]
    async fn read_applies_the_same_data_dir_check() {
        let fixture = Fixture::new();
        let bot = Bot::new("0:test");
        let inside = fixture.root().join("bot/file.mp4");
        let outside = fixture.root().join("bot/../../secret");

        let read = fixture
            .files
            .read(&bot, inside.to_str().unwrap_or_default())
            .await;
        assert_eq!(assert_ok!(read), b"video");

        let read = fixture
            .files
            .read(&bot, outside.to_str().unwrap_or_default())
            .await;
        assert_matches!(read, Err(TelegramFileError::OutsideLocalDir { .. }));
    }

    #[tokio::test]
    async fn local_path_without_a_data_dir_is_rejected() {
        let fixture = Fixture::new();
        let file_path = fixture.root().join("bot/file.mp4");
        let result = TelegramFiles::default()
            .save(
                &Bot::new("0:test"),
                file_path.to_str().unwrap_or_default(),
                &fixture.dir.path().join("saved"),
            )
            .await;
        assert_matches!(result, Err(TelegramFileError::LocalDisabled { .. }));
    }

    #[tokio::test]
    async fn traversal_out_of_the_data_dir_is_rejected() {
        let fixture = Fixture::new();
        let saved = fixture.save(&fixture.root().join("bot/../../secret")).await;
        assert_matches!(saved, Err(TelegramFileError::OutsideLocalDir { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_out_of_the_data_dir_is_rejected() {
        let fixture = Fixture::new();
        let link = fixture.root().join("bot/link.mp4");
        assert_ok!(std::os::unix::fs::symlink(
            fixture.dir.path().join("secret"),
            &link
        ));
        let saved = fixture.save(&link).await;
        assert_matches!(saved, Err(TelegramFileError::OutsideLocalDir { .. }));
    }
}
