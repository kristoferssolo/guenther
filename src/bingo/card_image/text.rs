use crate::bingo::card_image::layout::Rect;
use image::{Rgba, RgbaImage};
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};

const LINE_GAP: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Font {
    pub size: Size,
    pub weight: Weight,
    pub scale: u32,
}

impl Font {
    pub const fn new(size: Size, weight: Weight) -> Self {
        Self {
            size,
            weight,
            scale: 1,
        }
    }

    pub const fn scaled(mut self, scale: u32) -> Self {
        self.scale = scale;
        self
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.size.height() * self.scale
    }

    #[must_use]
    pub fn character_width(self) -> u32 {
        u32::try_from(get_raster_width(self.weight.into(), self.size.into()))
            .expect("font raster width fits in u32")
            * self.scale
    }

    #[must_use]
    pub const fn line_height(self) -> u32 {
        self.height() + LINE_GAP
    }
}

impl Size {
    const fn height(self) -> u32 {
        match self {
            Self::Small => 16,
            Self::Medium => 24,
            Self::Large => 32,
        }
    }
}

impl From<Size> for RasterHeight {
    fn from(value: Size) -> Self {
        match value {
            Size::Small => Self::Size16,
            Size::Medium => Self::Size24,
            Size::Large => Self::Size32,
        }
    }
}

impl From<Weight> for FontWeight {
    fn from(value: Weight) -> Self {
        match value {
            Weight::Regular => Self::Regular,
            Weight::Bold => Self::Bold,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub lines: Vec<String>,
    pub font: Font,
    pub truncated: bool,
}

impl TextBlock {
    #[must_use]
    pub fn width(&self) -> u32 {
        let columns = self
            .lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or_default();
        u32::try_from(columns).expect("line length fits in u32") * self.font.character_width()
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        let line_count = u32::try_from(self.lines.len()).expect("line count fits in u32");
        line_count
            .saturating_mul(self.font.line_height())
            .saturating_sub(LINE_GAP)
    }
}

#[must_use]
pub fn fit_text(text: &str, bounds: Rect, fonts: &[Font]) -> TextBlock {
    assert!(!fonts.is_empty(), "font candidates cannot be empty");
    for &font in fonts {
        let columns = bounds.width / font.character_width();
        let lines = wrap_text(
            text,
            usize::try_from(columns).expect("column count fits in usize"),
        );
        if lines_fit(&lines, font, bounds.height) {
            return TextBlock {
                lines,
                font,
                truncated: false,
            };
        }
    }

    let font = *fonts.last().expect("font candidates are not empty");
    let columns = usize::try_from(bounds.width / font.character_width())
        .expect("column count fits in usize")
        .max(1);
    let mut lines = wrap_text(text, columns);
    let max_lines = usize::try_from((bounds.height + LINE_GAP) / font.line_height())
        .expect("line count fits in usize")
        .max(1);
    lines.truncate(max_lines);
    if let Some(last) = lines.last_mut() {
        let keep = columns.saturating_sub(1);
        *last = format!("{}…", last.chars().take(keep).collect::<String>());
    }
    TextBlock {
        lines,
        font,
        truncated: true,
    }
}

#[must_use]
pub fn wrap_text(text: &str, columns: usize) -> Vec<String> {
    if columns == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let required = word.chars().count() + usize::from(!current.is_empty());
            if current.chars().count() + required <= columns {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut remaining = word;
            while remaining.chars().count() > columns {
                let split = remaining
                    .char_indices()
                    .nth(columns)
                    .map_or(remaining.len(), |(index, _)| index);
                lines.push(remaining[..split].to_owned());
                remaining = &remaining[split..];
            }
            current.push_str(remaining);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn draw_text_block(image: &mut RgbaImage, bounds: Rect, block: &TextBlock, color: [u8; 4]) {
    debug_assert!(block.width() <= bounds.width);
    debug_assert!(block.height() <= bounds.height);
    let total_height = block.height();
    let mut y = bounds.y + bounds.height.saturating_sub(total_height) / 2;
    for line in &block.lines {
        let width = u32::try_from(line.chars().count()).expect("line length fits in u32")
            * block.font.character_width();
        let mut x = bounds.x + bounds.width.saturating_sub(width) / 2;
        for character in line.chars() {
            draw_character(image, bounds, x, y, character, block.font, color);
            x += block.font.character_width();
        }
        y += block.font.line_height();
    }
}

fn lines_fit(lines: &[String], font: Font, height: u32) -> bool {
    let line_count = u32::try_from(lines.len()).expect("line count fits in u32");
    line_count
        .saturating_mul(font.line_height())
        .saturating_sub(LINE_GAP)
        <= height
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
    let raster = get_raster(character, font.weight.into(), font.size.into())
        .or_else(|| get_raster('?', font.weight.into(), font.size.into()))
        .expect("bundled font includes the fallback character");
    for (source_y, row) in raster.raster().iter().enumerate() {
        for (source_x, &coverage) in row.iter().enumerate() {
            for scale_y in 0..font.scale {
                for scale_x in 0..font.scale {
                    let target_x = x
                        + u32::try_from(source_x).expect("font x coordinate fits in u32")
                            * font.scale
                        + scale_x;
                    let target_y = y
                        + u32::try_from(source_y).expect("font y coordinate fits in u32")
                            * font.scale
                        + scale_y;
                    if target_x < clip.x
                        || target_y < clip.y
                        || target_x >= clip.x + clip.width
                        || target_y >= clip.y + clip.height
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
    let coverage = u16::from(coverage);
    let inverse = 255 - coverage;
    for (background, foreground) in background.0[..3].iter_mut().zip(foreground) {
        let blended = (u16::from(*background) * inverse + u16::from(foreground) * coverage) / 255;
        *background = u8::try_from(blended).expect("blended color channel fits in u8");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_BOUNDS: Rect = Rect::new(0, 0, 216, 216);
    const CELL_FONTS: [Font; 3] = [
        Font::new(Size::Large, Weight::Regular),
        Font::new(Size::Medium, Weight::Regular),
        Font::new(Size::Small, Weight::Regular),
    ];

    #[test]
    fn wraps_words_and_unbroken_text() {
        assert_eq!(wrap_text("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap_text("abcdefgh", 3), ["abc", "def", "gh"]);
    }

    #[test]
    fn fits_typical_128_character_entries() {
        let text = "Safety car during a wet race creates an unexpected strategy gamble for every driver on the grid before the restart";
        let text = format!("{text:<128}");
        let block = fit_text(text.trim(), CELL_BOUNDS, &CELL_FONTS);
        assert!(!block.truncated);
        assert!(block.width() <= CELL_BOUNDS.width);
        assert!(block.height() <= CELL_BOUNDS.height);
    }

    #[test]
    fn ellipsizes_text_that_cannot_fit_at_the_minimum_size() {
        let block = fit_text(&"x\n".repeat(128), CELL_BOUNDS, &CELL_FONTS);
        assert!(block.truncated);
        assert!(block.lines.last().is_some_and(|line| line.ends_with('…')));
        assert!(block.width() <= CELL_BOUNDS.width);
        assert!(block.height() <= CELL_BOUNDS.height);
    }

    #[test]
    fn wraps_titles_and_descriptions_within_their_bounds() {
        let title = fit_text(
            "A very long championship bingo game title",
            Rect::new(0, 0, 1_260, 150),
            &[
                Font::new(Size::Large, Weight::Bold).scaled(2),
                Font::new(Size::Large, Weight::Bold),
            ],
        );
        let description = fit_text(
            &"Welcome to a long season of racing. ".repeat(8),
            Rect::new(0, 0, 1_220, 220),
            &[Font::new(Size::Medium, Weight::Regular)],
        );
        assert!(title.width() <= 1_260 && title.height() <= 150);
        assert!(description.width() <= 1_220 && description.height() <= 220);
    }
}
