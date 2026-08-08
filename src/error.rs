use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] tokio::io::Error),

    #[error("Cobalt request failed: {0}")]
    CobaltRequest(#[source] reqwest::Error),

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

    #[error("No upcoming F1 race was found")]
    MissingF1Race,

    #[error("No matching F1 sessions were found")]
    MissingF1Sessions,

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
    #[inline]
    pub fn other(text: impl Into<String>) -> Self {
        Self::Other(text.into())
    }

    #[inline]
    pub fn validation_failed(text: impl Into<String>) -> Self {
        Self::ValidationFailed(text.into())
    }

    #[inline]
    pub fn env(text: impl Into<String>) -> Self {
        Self::EnvNotFound(text.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
