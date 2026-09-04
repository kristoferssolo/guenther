use thiserror::Error;

pub type Result<T> = std::result::Result<T, BingoError>;

#[derive(Debug, Error)]
pub enum BingoError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] teloxide::RequestError),

    #[error("Telegram file download failed: {0}")]
    Download(#[from] teloxide::DownloadError),

    #[error("Card image encoding failed: {0}")]
    Image(#[from] image::ImageError),

    #[error("Invalid bingo card layout: {0}")]
    InvalidCardLayout(String),

    #[error("{0}")]
    InvalidCommand(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("Only chat administrators can perform this action")]
    PermissionDenied,

    #[error("Only the card owner or a chat administrator can mark this card")]
    NotCardOwner,

    #[error("This game is not active, so its cards cannot be marked")]
    GameNotActive,

    #[error("The center cell is marked automatically and cannot be changed")]
    FreeCell,

    #[error("Telegram user ID {0} does not fit in the database")]
    UserIdOutOfRange(u64),

    #[error("Database contains invalid Telegram user ID {0}")]
    InvalidStoredUserId(i64),

    #[error("Database contains invalid bingo entry number {0}")]
    InvalidStoredEntryNumber(i64),
}

impl BingoError {
    pub const fn is_user_facing(&self) -> bool {
        matches!(
            self,
            Self::InvalidCommand(_)
                | Self::NotFound(_)
                | Self::Conflict(_)
                | Self::PermissionDenied
                | Self::NotCardOwner
                | Self::GameNotActive
                | Self::FreeCell
                | Self::UserIdOutOfRange(_)
        )
    }
}
