use crate::bingo::model::{EntryId, EntryNumber, GameId};

pub const MAX_ENTRY_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    pub number: EntryNumber,
    pub game_id: GameId,
    pub text: String,
}

pub fn normalize_entry(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    for (index, word) in text.split_whitespace().enumerate() {
        if index > 0 {
            normalized.push(' ');
        }
        normalized.extend(word.chars().flat_map(char::to_lowercase));
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace_and_case() {
        assert_eq!(normalize_entry("  Safety\tCAR  "), "safety car");
        assert_eq!(normalize_entry(""), "");
    }
}
