use crate::bingo::{
    error::{BingoError, Result},
    model::GameId,
};
use std::{fmt, str::FromStr};
use teloxide::types::ChatId;

pub const MAX_GAME_DESCRIPTION_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Draft,
    Active,
    Closed,
}

impl AsRef<str> for GameState {
    fn as_ref(&self) -> &str {
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
                "Database contains unknown game state `{unknown}`"
            ))),
        }
    }
}

impl fmt::Display for GameState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: GameId,
    pub chat_id: ChatId,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub center_text: String,
    pub state: GameState,
    pub is_default: bool,
}
