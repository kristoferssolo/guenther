pub const MAX_ENTRY_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    pub game_id: GameId,
    pub text: String,
}

#[must_use]
pub fn normalize_entry(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
use crate::bingo::model::{EntryId, GameId};
