use super::{CELL_COUNT, CardId, EntryNumber, GRID_SIDE, Game, KnownUser, Position};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardCell {
    pub position: Position,
    pub text: String,
    pub marked: bool,
    pub is_free: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub id: CardId,
    pub game: Game,
    pub owner: KnownUser,
    pub bingo_announced: bool,
    pub cells: Vec<CardCell>,
}

impl Card {
    pub fn has_bingo(&self) -> bool {
        has_bingo(&self.cells)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedCell {
    pub entry_number: Option<EntryNumber>,
    pub marked: bool,
    pub is_free: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleResult {
    pub card: Card,
    pub newly_completed: bool,
}

pub fn has_bingo(cells: &[CardCell]) -> bool {
    if cells.len() != CELL_COUNT {
        return false;
    }

    let (rows, remainder) = cells.as_chunks::<GRID_SIDE>();
    debug_assert!(remainder.is_empty());
    rows.iter().any(|row| row.iter().all(|cell| cell.marked))
        || (0..GRID_SIDE).any(|column| {
            rows.iter()
                .all(|row| row.get(column).is_some_and(|cell| cell.marked))
        })
        || rows
            .iter()
            .enumerate()
            .all(|(row, cells)| cells.get(row).is_some_and(|cell| cell.marked))
        || rows
            .iter()
            .zip((0..GRID_SIDE).rev())
            .all(|(cells, column)| cells.get(column).is_some_and(|cell| cell.marked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bingo::model::{FREE_POSITION, Position};
    use claims::assert_ok;

    fn cells(marked_positions: &[usize]) -> Vec<CardCell> {
        (0..CELL_COUNT)
            .map(|index| {
                let position = assert_ok!(Position::try_from(index));
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
