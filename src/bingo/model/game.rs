use crate::bingo::error::{BingoError, Result};
use std::{fmt, str::FromStr};
use teloxide::types::ChatId;

pub const MAX_GAME_DESCRIPTION_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Draft,
    Active,
    Closed,
}

impl GameState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Closed => "closed",
        }
    }
}

impl FromStr for GameState {
    type Err = BingoError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            unknown => Err(BingoError::Conflict(format!(
                "database contains unknown game state `{unknown}`"
            ))),
        }
    }
}

impl fmt::Display for GameState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: i64,
    pub chat_id: ChatId,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub center_text: String,
    pub state: GameState,
    pub is_default: bool,
}
