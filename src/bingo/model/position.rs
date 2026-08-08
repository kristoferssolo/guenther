use crate::bingo::error::{BingoError, Result};
use std::{fmt, str::FromStr};

pub const GRID_SIDE: usize = 5;
pub const CELL_COUNT: usize = GRID_SIDE * GRID_SIDE;
pub const FREE_POSITION: usize = 12;
pub const REQUIRED_ENTRIES: usize = CELL_COUNT - 1;
const GRID_SIDE_U8: u8 = 5;
const CELL_COUNT_U8: u8 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(u8);

impl Position {
    pub const FREE: Self = Self(12);

    #[must_use]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    pub fn iter() -> impl ExactSizeIterator<Item = Self> {
        (0..CELL_COUNT_U8).map(Self)
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
        u8::try_from(index)
            .ok()
            .filter(|index| *index < CELL_COUNT_U8)
            .map(Self)
            .ok_or_else(|| BingoError::InvalidCommand(format!("invalid cell position `{index}`")))
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
        let mut bytes = normalized.bytes();
        let coordinates = (bytes.next(), bytes.next(), bytes.next());
        let (Some(row), Some(column), None) = coordinates else {
            return Err(BingoError::InvalidCommand(format!(
                "invalid cell `{raw}`; expected A1 through E5"
            )));
        };
        if !(b'A'..=b'E').contains(&row) || !(b'1'..=b'5').contains(&column) {
            return Err(BingoError::InvalidCommand(format!(
                "invalid cell `{raw}`; expected A1 through E5"
            )));
        }

        Ok(Self(
            row.saturating_sub(b'A')
                .saturating_mul(GRID_SIDE_U8)
                .saturating_add(column.saturating_sub(b'1')),
        ))
    }
}

impl fmt::Display for Position {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let row = char::from(b'A'.saturating_add(self.0 / GRID_SIDE_U8));
        let column = (self.0 % GRID_SIDE_U8).saturating_add(1);
        write!(formatter, "{row}{column}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok_eq};

    #[test]
    fn coordinates_round_trip() {
        assert_ok_eq!("a1".parse::<Position>(), Position(0));
        assert_ok_eq!("E5".parse::<Position>(), Position(24));
        assert_err!("F1".parse::<Position>());
    }
}
