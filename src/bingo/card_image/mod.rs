mod draw;
mod layout;
mod text;

use crate::bingo::{
    card_image::{
        draw::{draw_circle, draw_grid, draw_text_block},
        layout::{
            BLACK, CELL_PADDING, DESCRIPTION, FOOTER, FREE_GOLD, IMAGE_HEIGHT, IMAGE_WIDTH,
            MARKED_RED, TITLE, WHITE, cell_rect,
        },
        text::{Font, Size, Weight, fit_text},
    },
    error::Result,
    model::Card,
};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

const TITLE_FONTS: [Font; 3] = [
    Font::new(Size::Large, Weight::Bold).scaled(2),
    Font::new(Size::Large, Weight::Bold),
    Font::new(Size::Medium, Weight::Bold),
];
const DESCRIPTION_FONTS: [Font; 2] = [
    Font::new(Size::Large, Weight::Regular),
    Font::new(Size::Medium, Weight::Regular),
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
    let mut image = RgbaImage::from_pixel(IMAGE_WIDTH, IMAGE_HEIGHT, Rgba(WHITE));

    let title = fit_text(&card.game.name, TITLE, &TITLE_FONTS);
    draw_text_block(&mut image, TITLE, &title, BLACK);
    if !card.game.description.is_empty() {
        let description = fit_text(&card.game.description, DESCRIPTION, &DESCRIPTION_FONTS);
        draw_text_block(&mut image, DESCRIPTION, &description, BLACK);
    }

    for cell in &card.cells {
        let color = if cell.is_free {
            FREE_GOLD
        } else if cell.marked {
            MARKED_RED
        } else {
            continue;
        };
        draw_circle(&mut image, cell_rect(cell.position), color);
    }
    draw_grid(&mut image);
    for cell in &card.cells {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bingo::model::{CardCell, CardId, Game, GameId, GameState, KnownUser, Position};
    use claims::assert_ok;
    use image::GenericImageView;
    use teloxide::types::{ChatId, UserId};

    fn card(description: &str) -> Card {
        let cells = std::array::from_fn(|index| CardCell {
            position: assert_ok!(Position::try_from(index)),
            text: if index == 0 {
                "A 128-character entry that remains complete in the companion text while the image renderer carefully wraps or truncates it as needed."
                    .to_owned()
            } else {
                format!("Cell {index}")
            },
            marked: index == 1 || index == 12,
            is_free: index == 12,
        });
        Card {
            id: CardId::from(42),
            game: Game {
                id: GameId::from(1),
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
    fn encodes_a_png_with_fixed_dimensions() {
        let bytes = assert_ok!(render_card_png(&card("Welcome to the 2026 season.")));
        let decoded = assert_ok!(image::load_from_memory_with_format(
            &bytes,
            ImageFormat::Png
        ));
        assert_eq!(decoded.dimensions(), (IMAGE_WIDTH, IMAGE_HEIGHT));
    }

    #[test]
    fn renders_cards_with_empty_descriptions() {
        assert_ok!(render_card_png(&card("")));
    }

    #[test]
    fn clips_circles_to_image_bounds() {
        let mut image = RgbaImage::new(8, 8);
        draw_circle(&mut image, layout::Rect::new(0, 0, 4, 4), MARKED_RED);
        assert_eq!(image.get_pixel(0, 0), &Rgba(MARKED_RED));
    }

    #[test]
    #[ignore = "writes a local visual preview"]
    fn bingo_card_preview() {
        let preview = card(
            "Welcome to the 2026 season. Mark each event as it happens and complete any row, column, or diagonal.",
        );
        let bytes = assert_ok!(render_card_png(&preview));
        assert_ok!(std::fs::create_dir_all("target"));
        assert_ok!(std::fs::write("target/bingo-card-preview.png", bytes));
    }
}
