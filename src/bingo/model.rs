use teloxide::types::{ChatId, UserId};

use crate::bingo::error::{BingoError, Result};
use std::{fmt, str::FromStr};

pub const GRID_SIDE: usize = 5;
pub const CELL_COUNT: usize = GRID_SIDE * GRID_SIDE;
pub const FREE_POSITION: usize = 12;
pub const REQUIRED_ENTRIES: usize = CELL_COUNT - 1;
pub const MAX_ENTRY_CHARS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(u8);

impl Position {
    pub const FREE: Self = Self(12);

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub fn iter() -> impl ExactSizeIterator<Item = Self> {
        let cell_count = u8::try_from(CELL_COUNT).expect("bingo cell count fits in a u8");
        (0..cell_count).map(Self)
    }
}

impl TryFrom<i64> for Position {
    type Error = BingoError;

    fn try_from(index: i64) -> Result<Self> {
        usize::try_from(index)
            .map_err(|_| BingoError::InvalidCommand(format!("invalid cell position `{index}`")))?
            .try_into()
    }
}

impl TryFrom<usize> for Position {
    type Error = BingoError;

    fn try_from(index: usize) -> Result<Self> {
        if index < CELL_COUNT {
            Ok(Self(
                u8::try_from(index).expect("validated bingo position fits in a u8"),
            ))
        } else {
            Err(BingoError::InvalidCommand(format!(
                "invalid cell position `{index}`"
            )))
        }
    }
}

impl From<Position> for i64 {
    fn from(position: Position) -> Self {
        Self::from(position.0)
    }
}

impl FromStr for Position {
    type Err = BingoError;

    fn from_str(raw: &str) -> Result<Self> {
        let normalized = raw.trim().to_ascii_uppercase();
        let bytes = normalized.as_bytes();
        if bytes.len() != 2
            || !(b'A'..=b'E').contains(&bytes[0])
            || !(b'1'..=b'5').contains(&bytes[1])
        {
            return Err(BingoError::InvalidCommand(format!(
                "invalid cell `{raw}`; expected A1 through E5"
            )));
        }

        let row = bytes[0] - b'A';
        let column = bytes[1] - b'1';
        let grid_side = u8::try_from(GRID_SIDE).expect("bingo grid side fits in a u8");
        Ok(Self(row * grid_side + column))
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let grid_side = u8::try_from(GRID_SIDE).expect("bingo grid side fits in a u8");
        let row = char::from(b'A' + self.0 / grid_side);
        let column = self.0 % grid_side + 1;
        write!(f, "{row}{column}")
    }
}

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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownUser {
    pub user_id: UserId,
    pub username: Option<String>,
    pub display_name: String,
}

impl fmt::Display for KnownUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.username {
            Some(name) => write!(f, "@{name}"),
            None => f.write_str(&self.display_name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: i64,
    pub chat_id: ChatId,
    pub slug: String,
    pub name: String,
    pub center_text: String,
    pub state: GameState,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub game_id: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCell {
    pub position: Position,
    pub text: String,
    pub marked: bool,
    pub is_free: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: i64,
    pub game: Game,
    pub owner: KnownUser,
    pub bingo_announced: bool,
    pub cells: Vec<CardCell>,
}

impl Card {
    #[must_use]
    pub fn has_bingo(&self) -> bool {
        has_bingo(&self.cells)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCell {
    pub text: String,
    pub marked: bool,
    pub is_free: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleResult {
    pub card: Card,
    pub newly_completed: bool,
}

#[must_use]
pub fn normalize_entry(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[must_use]
pub fn has_bingo(cells: &[CardCell]) -> bool {
    if cells.len() != CELL_COUNT {
        return false;
    }

    let marked = |position: usize| cells[position].marked;
    (0..GRID_SIDE).any(|row| (0..GRID_SIDE).all(|column| marked(row * GRID_SIDE + column)))
        || (0..GRID_SIDE).any(|column| (0..GRID_SIDE).all(|row| marked(row * GRID_SIDE + column)))
        || (0..GRID_SIDE).all(|index| marked(index * (GRID_SIDE + 1)))
        || (0..GRID_SIDE).all(|index| marked((index + 1) * (GRID_SIDE - 1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok_eq};

    fn cells(marked_positions: &[usize]) -> Vec<CardCell> {
        (0..CELL_COUNT)
            .map(|index| {
                let position = Position::try_from(index).expect("test position is valid");
                CardCell {
                    position,
                    text: position.to_string(),
                    marked: marked_positions.contains(&index),
                    is_free: index == FREE_POSITION,
                }
            })
            .collect()
    }

    #[test]
    fn coordinates_round_trip() {
        assert_ok_eq!("a1".parse::<Position>(), Position(0));
        assert_ok_eq!("E5".parse::<Position>(), Position(24));
        assert_err!("F1".parse::<Position>());
    }

    #[test]
    fn detects_rows_columns_and_diagonals() {
        assert!(has_bingo(&cells(&[0, 1, 2, 3, 4])));
        assert!(has_bingo(&cells(&[2, 7, 12, 17, 22])));
        assert!(has_bingo(&cells(&[0, 6, 12, 18, 24])));
        assert!(has_bingo(&cells(&[4, 8, 12, 16, 20])));
        assert!(!has_bingo(&cells(&[0, 1, 2, 3])));
    }
}
