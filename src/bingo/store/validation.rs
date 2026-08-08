use crate::bingo::{
    error::{BingoError, Result},
    model::{Game, GameState},
};

pub(in crate::bingo::store) fn validate_slug(slug: &str) -> Result<()> {
    let valid = (2..=50).contains(&slug.len())
        && !slug.bytes().all(|byte| byte.is_ascii_digit())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(BingoError::InvalidCommand(
            "game slugs must be 2-50 lowercase letters, numbers, or hyphens and cannot be purely numeric"
                .to_owned(),
        ))
    }
}

pub(in crate::bingo::store) fn validate_nonempty(
    text: &str,
    label: &str,
    max_chars: usize,
) -> Result<()> {
    let length = text.trim().chars().count();
    if length == 0 || length > max_chars {
        Err(BingoError::InvalidCommand(format!(
            "{label} must contain between 1 and {max_chars} characters"
        )))
    } else {
        Ok(())
    }
}

pub(in crate::bingo::store) fn ensure_active(game: &Game) -> Result<()> {
    if game.state == GameState::Active {
        Ok(())
    } else {
        Err(BingoError::GameNotActive)
    }
}

pub(in crate::bingo::store) fn ensure_editable(game: &Game) -> Result<()> {
    if game.state == GameState::Closed {
        Err(BingoError::Conflict(
            "closed games cannot be changed".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub(in crate::bingo::store) fn require_changed(
    rows_affected: u64,
    message: impl FnOnce() -> String,
) -> Result<()> {
    if rows_affected == 0 {
        Err(BingoError::NotFound(message()))
    } else {
        Ok(())
    }
}

pub(in crate::bingo::store) fn map_unique(
    error: sqlx::Error,
    message: impl FnOnce() -> String,
) -> BingoError {
    if matches!(&error, sqlx::Error::Database(database) if database.is_unique_violation()) {
        BingoError::Conflict(message())
    } else {
        BingoError::Database(error)
    }
}
