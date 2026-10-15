use crate::download::DownloadError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] tokio::io::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] teloxide::RequestError),

    #[error(transparent)]
    Download(#[from] DownloadError),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(text: impl Into<String>) -> Self {
        Self::Other(text.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
