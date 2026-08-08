pub const MAX_ENTRY_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub game_id: i64,
    pub text: String,
}

#[must_use]
pub fn normalize_entry(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
