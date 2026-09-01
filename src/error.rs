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

    #[error("Unknown media kind")]
    UnknownMediaKind,

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Task join failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("Environment variable `{0}` was not found")]
    EnvNotFound(String),

    #[error("Failed to fetch the F1 schedule: {0}")]
    FetchF1Schedule(#[source] reqwest::Error),

    #[error("Failed to decode the F1 schedule: {0}")]
    DecodeF1Schedule(#[source] reqwest::Error),

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

    #[error("No upcoming F1 race was found")]
    MissingF1Race,

    #[error("No matching F1 sessions were found")]
    MissingF1Sessions,

    #[error("Failed to fetch the F1 standings: {0}")]
    FetchF1Standings(#[source] reqwest::Error),

    #[error("Failed to decode the F1 standings: {0}")]
    DecodeF1Standings(#[source] reqwest::Error),

    #[error("No F1 standings were found")]
    MissingF1Standings,

    #[error("Failed to parse F1 session time `{raw}`")]
    ParseF1SessionTime {
        raw: String,
        #[source]
        source: chrono::ParseError,
    },

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(text: impl Into<String>) -> Self {
        Self::Other(text.into())
    }

    pub fn validation_failed(text: impl Into<String>) -> Self {
        Self::ValidationFailed(text.into())
    }

    pub fn env(text: impl Into<String>) -> Self {
        Self::EnvNotFound(text.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
