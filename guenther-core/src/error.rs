use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] tokio::io::Error),

    #[error("yt-dlp failed: {0}")]
    YTDLPFailed(String),

    #[error("no media found")]
    NoMediaFound,

    #[error("unknown media kind")]
    UnknownMediaKind,

    #[error("validation failed: {0}")]
    ValidationFailed(String),

    #[error("join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("environment variable `{0}` not found")]
    EnvNotFound(String),

    #[error("failed to fetch F1 schedule: {0}")]
    FetchF1Schedule(#[source] reqwest::Error),

    #[error("failed to decode F1 schedule: {0}")]
    DecodeF1Schedule(#[source] reqwest::Error),

    #[error("no upcoming F1 race found")]
    MissingF1Race,

    #[error("no matching F1 sessions found")]
    MissingF1Sessions,

    #[error("failed to parse F1 session time `{raw}`")]
    ParseF1SessionTime {
        raw: String,
        #[source]
        source: time::error::Parse,
    },

    #[error("failed to format F1 session time: {0}")]
    FormatF1SessionTime(#[source] time::error::Format),

    #[error("other: {0}")]
    Other(String),
}

impl Error {
    #[inline]
    pub fn other(text: impl Into<String>) -> Self {
        Self::Other(text.into())
    }

    #[inline]
    pub fn ytdlp_failed(text: impl Into<String>) -> Self {
        Self::YTDLPFailed(text.into())
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
