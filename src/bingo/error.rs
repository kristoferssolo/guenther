use thiserror::Error;

pub type Result<T> = std::result::Result<T, BingoError>;

#[derive(Debug, Error)]
pub enum BingoError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] teloxide::RequestError),

    #[error("{0}")]
    InvalidCommand(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("only chat administrators can do that")]
    PermissionDenied,

    #[error("only the card owner can mark this card")]
    NotCardOwner,

    #[error("this game is not active")]
    GameNotActive,

    #[error("the center cell is marked automatically")]
    FreeCell,

    #[error("Telegram user ID {0} does not fit in the database")]
    UserIdOutOfRange(u64),

    #[error("database contains invalid Telegram user ID {0}")]
    InvalidStoredUserId(i64),
}

impl BingoError {
    #[must_use]
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
