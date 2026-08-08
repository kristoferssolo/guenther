use crate::bingo::card_image::layout::Rect;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster_width};

const LINE_GAP: u32 = 5;
const DEFAULT_FONT: Font = Font::new(Size::Small, Weight::Regular);

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
        self.size.height().saturating_mul(self.scale)
    }

    #[must_use]
    pub fn character_width(self) -> u32 {
        u32::try_from(get_raster_width(self.weight.into(), self.size.into()))
            .unwrap_or(u32::MAX)
            .saturating_mul(self.scale)
    }

    #[must_use]
    pub const fn line_height(self) -> u32 {
        self.height().saturating_add(LINE_GAP)
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
        u32::try_from(columns)
            .unwrap_or(u32::MAX)
            .saturating_mul(self.font.character_width())
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        let line_count = u32::try_from(self.lines.len()).unwrap_or(u32::MAX);
        line_count
            .saturating_mul(self.font.line_height())
            .saturating_sub(LINE_GAP)
    }
}

#[must_use]
pub fn fit_text(text: &str, bounds: Rect, fonts: &[Font]) -> TextBlock {
    for &font in fonts {
        let columns = column_capacity(font, bounds);
        if columns == 0 {
            continue;
        }
        let lines = layout(text, font, bounds);
        if lines_fit(&lines, font, bounds.height) {
            return TextBlock {
                lines,
                font,
                truncated: false,
            };
        }
    }

    let font = fonts.last().copied().unwrap_or(DEFAULT_FONT);
    let columns = column_capacity(font, bounds);
    let max_lines = usize::try_from(bounds.height.saturating_add(LINE_GAP) / font.line_height())
        .unwrap_or(usize::MAX);
    if columns == 0 || max_lines == 0 {
        return TextBlock {
            lines: Vec::new(),
            font,
            truncated: !text.is_empty(),
        };
    }

    let mut lines = layout(text, font, bounds);
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

fn layout(text: &str, font: Font, bounds: Rect) -> Vec<String> {
    wrap_text(text, column_capacity(font, bounds))
}

fn column_capacity(font: Font, bounds: Rect) -> usize {
    let character_width = font.character_width();
    if character_width == 0 {
        return 0;
    }
    usize::try_from(bounds.width / character_width).unwrap_or(usize::MAX)
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

fn lines_fit(lines: &[String], font: Font, height: u32) -> bool {
    let line_count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    line_count
        .saturating_mul(font.line_height())
        .saturating_sub(LINE_GAP)
        <= height
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
    fn reports_truncation_when_no_character_fits() {
        let font = Font::new(Size::Small, Weight::Regular);
        let bounds = Rect::new(
            0,
            0,
            font.character_width().saturating_sub(1),
            font.height(),
        );
        let block = fit_text("x", bounds, &[font]);

        assert!(block.truncated);
        assert!(block.lines.is_empty());
        assert!(block.width() <= bounds.width);
        assert!(block.height() <= bounds.height);
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
