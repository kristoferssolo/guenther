use crate::bingo::{
    error::{BingoError, Result},
    model::{Game, GameState},
};

pub fn validate_slug(slug: &str) -> Result<()> {
    let valid = (2..=50).contains(&slug.len())
        && !slug.bytes().all(|byte| byte.is_ascii_digit())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(BingoError::InvalidCommand(
            "Game slugs must be 2-50 lowercase letters, numbers, or hyphens and cannot be purely numeric"
                .to_owned(),
        ))
    }
}

pub fn validate_nonempty(text: &str, label: &str, max_chars: usize) -> Result<()> {
    let length = text.trim().chars().count();
    if length == 0 || length > max_chars {
        Err(BingoError::InvalidCommand(format!(
            "The {label} must contain between 1 and {max_chars} characters"
        )))
    } else {
        Ok(())
    }
}

pub fn ensure_active(game: &Game) -> Result<()> {
    if game.state == GameState::Active {
        Ok(())
    } else {
        Err(BingoError::GameNotActive)
    }
}

pub fn ensure_editable(game: &Game) -> Result<()> {
    if game.state == GameState::Closed {
        Err(BingoError::Conflict(
            "Closed games cannot be changed".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub fn require_changed(rows_affected: u64, message: impl FnOnce() -> String) -> Result<()> {
    if rows_affected == 0 {
        Err(BingoError::NotFound(message()))
    } else {
        Ok(())
    }
}

pub fn map_unique(error: sqlx::Error, message: impl FnOnce() -> String) -> BingoError {
    if matches!(&error, sqlx::Error::Database(database) if database.is_unique_violation()) {
        BingoError::Conflict(message())
    } else {
        BingoError::Database(error)
    }
}
