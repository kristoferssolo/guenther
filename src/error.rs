use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] tokio::io::Error),

    #[error("Cobalt request failed: {0}")]
    CobaltRequest(#[source] reqwest::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] teloxide::RequestError),

    #[error("Cobalt rejected the download: {0}")]
    CobaltRejected(String),

    #[error("Cobalt returned unsupported local processing instructions")]
    CobaltLocalProcessing,

    #[error("No media found")]
    NoMediaFound,

    #[error("Failed to extract the tweet ID")]
    InvalidTwitterUrl,

    #[error("No downloadable images were found in this post")]
    MissingTwitterImages,

    #[error("Failed to build the HTTP client: {0}")]
    BuildHttpClient(#[source] reqwest::Error),

    #[error("Failed to fetch Twitter syndication data: {0}")]
    FetchTwitterSyndication(#[source] reqwest::Error),

    #[error("Failed to parse Twitter syndication data: {0}")]
    ParseTwitterSyndication(#[source] reqwest::Error),

    #[error("Failed to download the Twitter image: {0}")]
    DownloadTwitterImage(#[source] reqwest::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(text: impl Into<String>) -> Self {
        Self::Other(text.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
