use super::{CELL_COUNT, GRID_SIDE, Game, KnownUser, Position};

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
    pub entry_id: Option<i64>,
    pub marked: bool,
    pub is_free: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleResult {
    pub card: Card,
    pub newly_completed: bool,
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
    use crate::bingo::model::{FREE_POSITION, Position};

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
    fn detects_rows_columns_and_diagonals() {
        assert!(has_bingo(&cells(&[0, 1, 2, 3, 4])));
        assert!(has_bingo(&cells(&[2, 7, 12, 17, 22])));
        assert!(has_bingo(&cells(&[0, 6, 12, 18, 24])));
        assert!(has_bingo(&cells(&[4, 8, 12, 16, 20])));
        assert!(!has_bingo(&cells(&[0, 1, 2, 3])));
    }
}
