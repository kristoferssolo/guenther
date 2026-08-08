use crate::bingo::error::{BingoError, Result};
use std::{fmt, str::FromStr};

pub const GRID_SIDE: usize = 5;
pub const CELL_COUNT: usize = GRID_SIDE * GRID_SIDE;
pub const FREE_POSITION: usize = 12;
pub const REQUIRED_ENTRIES: usize = CELL_COUNT - 1;

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
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let grid_side = u8::try_from(GRID_SIDE).expect("bingo grid side fits in a u8");
        let row = char::from(b'A' + self.0 / grid_side);
        let column = self.0 % grid_side + 1;
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
