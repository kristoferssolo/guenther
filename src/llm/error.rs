use reqwest::StatusCode;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("OpenRouter request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("OpenRouter returned {status}: {message}")]
    Api { status: StatusCode, message: String },

    #[error("OpenRouter provider error: {0}")]
    Provider(String),

    #[error("OpenRouter response contained no text")]
    EmptyResponse,

    #[error("Model output is not a usable caption: {0}")]
    UnusableCaption(String),

    #[error("I/O error while preparing media: {0}")]
    Io(#[from] io::Error),

    #[error("Failed to run {program}: {source}")]
    Spawn {
        program: &'static str,
        #[source]
        source: io::Error,
    },

    #[error("ffmpeg failed to extract a frame: {0}")]
    Ffmpeg(String),

    #[error("Unsupported media type `{0}` for the model")]
    UnsupportedMedia(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;
