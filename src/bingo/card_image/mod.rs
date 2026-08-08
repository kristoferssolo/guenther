mod layout;
mod text;

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

use crate::bingo::{
    error::{BingoError, Result},
    model::{CELL_COUNT, Card, CardCell},
};

use self::{
    layout::{
        BLACK, CELL_PADDING, CIRCLE_DIAMETER, CellFill, DESCRIPTION, FOOTER, FREE_GOLD, GRID,
        GRID_LINE_WIDTH, IMAGE_HEIGHT, IMAGE_WIDTH, MARKED_RED, Rect, TITLE, WHITE, cell_fill,
        cell_rect,
    },
    text::{Font, Size, Weight, draw_text_block, fit_text},
};

const TITLE_FONTS: [Font; 3] = [
    Font::new(Size::Large, Weight::Bold).scaled(2),
    Font::new(Size::Large, Weight::Bold),
    Font::new(Size::Medium, Weight::Bold),
];
const DESCRIPTION_FONTS: [Font; 2] = [
    Font::new(Size::Medium, Weight::Regular),
    Font::new(Size::Small, Weight::Regular),
];
const CELL_FONTS: [Font; 3] = [
    Font::new(Size::Large, Weight::Regular),
    Font::new(Size::Medium, Weight::Regular),
    Font::new(Size::Small, Weight::Regular),
];
const FOOTER_FONTS: [Font; 2] = [
    Font::new(Size::Medium, Weight::Regular),
    Font::new(Size::Small, Weight::Regular),
];

pub fn render_card_png(card: &Card) -> Result<Vec<u8>> {
    let cells = ordered_cells(&card.cells)?;
    let mut image = RgbaImage::from_pixel(IMAGE_WIDTH, IMAGE_HEIGHT, Rgba(WHITE));

    let title = fit_text(&card.game.name, TITLE, &TITLE_FONTS);
    draw_text_block(&mut image, TITLE, &title, BLACK);
    if !card.game.description.is_empty() {
        let description = fit_text(&card.game.description, DESCRIPTION, &DESCRIPTION_FONTS);
        draw_text_block(&mut image, DESCRIPTION, &description, BLACK);
    }

    for cell in cells {
        let bounds = cell_rect(cell.position);
        match cell_fill(cell) {
            CellFill::None => {}
            CellFill::Marked => draw_circle(&mut image, bounds, MARKED_RED),
            CellFill::Free => draw_circle(&mut image, bounds, FREE_GOLD),
        }
    }
    draw_grid(&mut image);
    for cell in cells {
        let bounds = cell_rect(cell.position).inset(CELL_PADDING);
        let text = fit_text(&cell.text, bounds, &CELL_FONTS);
        draw_text_block(&mut image, bounds, &text, BLACK);
    }

    let footer_text = format!("{}  ·  Card #{}", card.owner, card.id);
    let footer = fit_text(&footer_text, FOOTER, &FOOTER_FONTS);
    draw_text_block(&mut image, FOOTER, &footer, BLACK);

    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image).write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}

fn ordered_cells(cells: &[CardCell]) -> Result<[&CardCell; CELL_COUNT]> {
    if cells.len() != CELL_COUNT {
        return Err(BingoError::InvalidCardLayout(format!(
            "expected {CELL_COUNT} cells, found {}",
            cells.len()
        )));
    }
    let mut ordered = [None; CELL_COUNT];
    for cell in cells {
        let index = cell.position.index();
        if ordered[index].replace(cell).is_some() {
            return Err(BingoError::InvalidCardLayout(format!(
                "cell {} appears more than once",
                cell.position
            )));
        }
    }
    ordered
        .map(|cell| cell.ok_or_else(|| BingoError::InvalidCardLayout("missing cell".to_owned())))
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| BingoError::InvalidCardLayout("incorrect cell count".to_owned()))
}

fn draw_circle(image: &mut RgbaImage, bounds: Rect, color: [u8; 4]) {
    let radius = i64::from(CIRCLE_DIAMETER / 2);
    let center_x = i64::from(bounds.x + bounds.width / 2);
    let center_y = i64::from(bounds.y + bounds.height / 2);
    for y in center_y - radius..=center_y + radius {
        for x in center_x - radius..=center_x + radius {
            let dx = x - center_x;
            let dy = y - center_y;
            if dx * dx + dy * dy <= radius * radius {
                let x = u32::try_from(x).expect("circle x coordinate is nonnegative");
                let y = u32::try_from(y).expect("circle y coordinate is nonnegative");
                image.put_pixel(x, y, Rgba(color));
            }
        }
    }
}

fn draw_grid(image: &mut RgbaImage) {
    for offset in 0..=5 {
        let offset = u32::try_from(offset).expect("grid offset fits in u32");
        let x = GRID.x + offset * layout::CELL_SIZE;
        let y = GRID.y + offset * layout::CELL_SIZE;
        draw_filled_rect(
            image,
            Rect::new(
                x.saturating_sub(GRID_LINE_WIDTH / 2),
                GRID.y,
                GRID_LINE_WIDTH,
                GRID.height,
            ),
            BLACK,
        );
        draw_filled_rect(
            image,
            Rect::new(
                GRID.x,
                y.saturating_sub(GRID_LINE_WIDTH / 2),
                GRID.width,
                GRID_LINE_WIDTH,
            ),
            BLACK,
        );
    }
}

fn draw_filled_rect(image: &mut RgbaImage, bounds: Rect, color: [u8; 4]) {
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            image.put_pixel(x, y, Rgba(color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bingo::model::{Game, GameState, KnownUser, Position};
    use claims::{assert_err, assert_ok};
    use image::GenericImageView;
    use teloxide::types::{ChatId, UserId};

    fn card(description: &str) -> Card {
        let mut cells = (0..CELL_COUNT)
            .map(|index| CardCell {
                position: Position::try_from(index).expect("valid test position"),
                text: if index == 0 {
                    "A 128-character entry that remains complete in the companion text while the image renderer carefully wraps or truncates it as needed."
                        .to_owned()
                } else {
                    format!("Cell {index}")
                },
                marked: index == 1 || index == 12,
                is_free: index == 12,
            })
            .collect::<Vec<_>>();
        cells.reverse();
        Card {
            id: 42,
            game: Game {
                id: 1,
                chat_id: ChatId(7),
                slug: "season-2026".to_owned(),
                name: "2026 Formula One Bingo".to_owned(),
                description: description.to_owned(),
                center_text: "LIGHTS OUT!".to_owned(),
                state: GameState::Active,
                is_default: true,
            },
            owner: KnownUser {
                user_id: UserId(9),
                username: Some("driver".to_owned()),
                display_name: "Driver".to_owned(),
            },
            bingo_announced: false,
            cells,
        }
    }

    #[test]
    fn validates_and_orders_cells_by_stored_position() {
        let card = card("");
        let ordered = ordered_cells(&card.cells).expect("valid card layout");
        assert_eq!(ordered[0].position.to_string(), "A1");
        assert_eq!(ordered[24].position.to_string(), "E5");

        let mut missing = card.clone();
        missing.cells.pop();
        assert_err!(ordered_cells(&missing.cells));

        let mut duplicate = card;
        duplicate.cells[0].position = duplicate.cells[1].position;
        assert_err!(ordered_cells(&duplicate.cells));
    }

    #[test]
    fn encodes_a_png_with_fixed_dimensions() {
        let bytes =
            render_card_png(&card("Welcome to the 2026 season.")).expect("render test bingo card");
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .expect("decode rendered PNG");
        assert_eq!(decoded.dimensions(), (IMAGE_WIDTH, IMAGE_HEIGHT));
    }

    #[test]
    fn renders_cards_with_empty_descriptions() {
        assert_ok!(render_card_png(&card("")));
    }

    #[test]
    #[ignore = "writes a local visual preview"]
    fn bingo_card_preview() {
        let preview = card(
            "Welcome to the 2026 season. Mark each event as it happens and complete any row, column, or diagonal.",
        );
        let bytes = render_card_png(&preview).expect("render preview card");
        std::fs::create_dir_all("target").expect("create target directory");
        std::fs::write("target/bingo-card-preview.png", bytes).expect("write preview image");
    }
}
