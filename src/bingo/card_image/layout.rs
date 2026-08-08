use crate::bingo::model::{CardCell, GRID_SIDE, Position};

pub const IMAGE_WIDTH: u32 = 1_400;
pub const IMAGE_HEIGHT: u32 = 2_000;

pub const WHITE: [u8; 4] = [255, 255, 255, 255];
pub const BLACK: [u8; 4] = [18, 18, 18, 255];
pub const MARKED_RED: [u8; 4] = [244, 184, 184, 255];
pub const FREE_GOLD: [u8; 4] = [244, 222, 164, 255];

pub const TITLE: Rect = Rect::new(70, 40, 1_260, 150);
pub const DESCRIPTION: Rect = Rect::new(90, 205, 1_220, 220);
pub const GRID: Rect = Rect::new(70, 460, 1_260, 1_260);
pub const FOOTER: Rect = Rect::new(70, 1_800, 1_260, 110);
pub const CELL_SIZE: u32 = 252;
pub const CELL_PADDING: u32 = 18;
pub const GRID_LINE_WIDTH: u32 = 4;
pub const CIRCLE_DIAMETER: u32 = CELL_SIZE * 3 / 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn inset(self, amount: u32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: self.width - amount * 2,
            height: self.height - amount * 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellFill {
    None,
    Marked,
    Free,
}

#[must_use]
pub fn cell_rect(position: Position) -> Rect {
    let index = u32::try_from(position.index()).expect("bingo position fits in u32");
    let grid_side = u32::try_from(GRID_SIDE).expect("bingo grid side fits in u32");
    let row = index / grid_side;
    let column = index % grid_side;
    Rect::new(
        GRID.x + column * CELL_SIZE,
        GRID.y + row * CELL_SIZE,
        CELL_SIZE,
        CELL_SIZE,
    )
}

#[must_use]
pub const fn cell_fill(cell: &CardCell) -> CellFill {
    if cell.is_free {
        CellFill::Free
    } else if cell.marked {
        CellFill::Marked
    } else {
        CellFill::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bingo::model::CardCell;

    fn cell(position: &str, marked: bool, is_free: bool) -> CardCell {
        CardCell {
            position: position.parse().expect("valid test position"),
            text: position.to_owned(),
            marked,
            is_free,
        }
    }

    #[test]
    fn maps_a1_through_e5_in_row_major_order() {
        assert_eq!(
            cell_rect("A1".parse().expect("valid position")),
            Rect::new(70, 460, 252, 252)
        );
        assert_eq!(
            cell_rect("A5".parse().expect("valid position")),
            Rect::new(1_078, 460, 252, 252)
        );
        assert_eq!(
            cell_rect("C3".parse().expect("valid position")),
            Rect::new(574, 964, 252, 252)
        );
        assert_eq!(
            cell_rect("E1".parse().expect("valid position")),
            Rect::new(70, 1_468, 252, 252)
        );
        assert_eq!(
            cell_rect("E5".parse().expect("valid position")),
            Rect::new(1_078, 1_468, 252, 252)
        );
    }

    #[test]
    fn selects_marked_and_free_cell_fills() {
        assert_eq!(cell_fill(&cell("A1", false, false)), CellFill::None);
        assert_eq!(cell_fill(&cell("A1", true, false)), CellFill::Marked);
        assert_eq!(cell_fill(&cell("C3", true, true)), CellFill::Free);
    }
}
