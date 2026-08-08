use crate::bingo::card_image::{
    layout::{BLACK, CIRCLE_DIAMETER, GRID, GRID_LINE_WIDTH, Rect},
    text::{Font, TextBlock},
};
use image::{Rgba, RgbaImage};
use noto_sans_mono_bitmap::get_raster;

pub fn draw_circle(image: &mut RgbaImage, bounds: Rect, color: [u8; 4]) {
    if image.width() == 0 || image.height() == 0 {
        return;
    }

    let radius = i64::from(CIRCLE_DIAMETER / 2);
    let center_x = i64::from(bounds.x.saturating_add(bounds.width / 2));
    let center_y = i64::from(bounds.y.saturating_add(bounds.height / 2));
    let start_x = center_x.saturating_sub(radius).max(0);
    let start_y = center_y.saturating_sub(radius).max(0);
    let end_x = center_x
        .saturating_add(radius)
        .min(i64::from(image.width().saturating_sub(1)));
    let end_y = center_y
        .saturating_add(radius)
        .min(i64::from(image.height().saturating_sub(1)));

    for y in start_y..=end_y {
        for x in start_x..=end_x {
            let dx = x.saturating_sub(center_x);
            let dy = y.saturating_sub(center_y);
            if dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
                <= radius.saturating_mul(radius)
                && let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y))
            {
                image.put_pixel(x, y, Rgba(color));
            }
        }
    }
}

pub fn draw_grid(image: &mut RgbaImage) {
    for offset in 0..=5_u32 {
        let x = GRID
            .x
            .saturating_add(offset.saturating_mul(super::layout::CELL_SIZE));
        let y = GRID
            .y
            .saturating_add(offset.saturating_mul(super::layout::CELL_SIZE));
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

pub fn draw_text_block(image: &mut RgbaImage, bounds: Rect, block: &TextBlock, color: [u8; 4]) {
    debug_assert!(block.width() <= bounds.width);
    debug_assert!(block.height() <= bounds.height);
    let total_height = block.height();
    let mut y = bounds
        .y
        .saturating_add(bounds.height.saturating_sub(total_height) / 2);
    for line in &block.lines {
        let width = u32::try_from(line.chars().count())
            .unwrap_or(u32::MAX)
            .saturating_mul(block.font.character_width());
        let mut x = bounds
            .x
            .saturating_add(bounds.width.saturating_sub(width) / 2);
        for character in line.chars() {
            draw_character(image, bounds, x, y, character, block.font, color);
            x = x.saturating_add(block.font.character_width());
        }
        y = y.saturating_add(block.font.line_height());
    }
}

fn draw_filled_rect(image: &mut RgbaImage, bounds: Rect, color: [u8; 4]) {
    let end_x = bounds.x.saturating_add(bounds.width).min(image.width());
    let end_y = bounds.y.saturating_add(bounds.height).min(image.height());
    for y in bounds.y.min(end_y)..end_y {
        for x in bounds.x.min(end_x)..end_x {
            image.put_pixel(x, y, Rgba(color));
        }
    }
}

fn draw_character(
    image: &mut RgbaImage,
    clip: Rect,
    x: u32,
    y: u32,
    character: char,
    font: Font,
    color: [u8; 4],
) {
    let Some(raster) = get_raster(character, font.weight.into(), font.size.into())
        .or_else(|| get_raster('?', font.weight.into(), font.size.into()))
    else {
        return;
    };
    for (source_y, row) in raster.raster().iter().enumerate() {
        for (source_x, &coverage) in row.iter().enumerate() {
            let Ok(source_x) = u32::try_from(source_x) else {
                continue;
            };
            let Ok(source_y) = u32::try_from(source_y) else {
                continue;
            };
            for scale_y in 0..font.scale {
                for scale_x in 0..font.scale {
                    let target_x = x
                        .saturating_add(source_x.saturating_mul(font.scale))
                        .saturating_add(scale_x);
                    let target_y = y
                        .saturating_add(source_y.saturating_mul(font.scale))
                        .saturating_add(scale_y);
                    if target_x < clip.x
                        || target_y < clip.y
                        || target_x >= clip.x.saturating_add(clip.width)
                        || target_y >= clip.y.saturating_add(clip.height)
                        || target_x >= image.width()
                        || target_y >= image.height()
                    {
                        continue;
                    }
                    blend_pixel(image.get_pixel_mut(target_x, target_y), color, coverage);
                }
            }
        }
    }
}

fn blend_pixel(background: &mut Rgba<u8>, foreground: [u8; 4], coverage: u8) {
    let [red, green, blue, alpha] = foreground;
    let coverage = u16::from(coverage).saturating_mul(u16::from(alpha)) / 255;
    let inverse = 255_u16.saturating_sub(coverage);
    for (background, foreground) in background.0.iter_mut().take(3).zip([red, green, blue]) {
        let blended = u16::from(*background)
            .saturating_mul(inverse)
            .saturating_add(u16::from(foreground).saturating_mul(coverage))
            / 255;
        *background = u8::try_from(blended).unwrap_or(u8::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending_respects_foreground_alpha() {
        let mut transparent = Rgba([255, 255, 255, 255]);
        blend_pixel(&mut transparent, [0, 0, 0, 0], 255);
        assert_eq!(transparent, Rgba([255, 255, 255, 255]));

        let mut opaque = Rgba([255, 255, 255, 255]);
        blend_pixel(&mut opaque, [0, 0, 0, 255], 255);
        assert_eq!(opaque, Rgba([0, 0, 0, 255]));
    }
}
