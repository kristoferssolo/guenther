mod parser;

#[cfg(test)]
mod tests;

use crate::bingo::error::Result;
use crate::bingo::model::{GameState, ImportedCell, Position};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BingoCommand {
    Help,
    Games,
    Entries {
        slug: Option<String>,
    },
    Get {
        slug: Option<String>,
        target: Option<String>,
    },
    Game(GameAdmin),
    Entry(EntryAdmin),
    Card(CardAdmin),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameAdmin {
    Create { slug: String, name: String },
    SetState { slug: String, state: GameState },
    SetDefault { slug: String },
    SetCenter { slug: String, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAdmin {
    Add { slug: Option<String>, text: String },
    Edit { entry_id: i64, text: String },
    Delete { entry_id: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardAdmin {
    Generate {
        slug: Option<String>,
        target: Option<String>,
        replace: bool,
    },
    Import {
        slug: String,
        target: Option<String>,
        cells: Vec<ImportedCell>,
        replace: bool,
    },
    Set {
        slug: String,
        target: Option<String>,
        position: Position,
        text: String,
    },
    Reset {
        slug: Option<String>,
        target: Option<String>,
    },
}

impl BingoCommand {
    pub fn parse(input: &str) -> Result<Self> {
        parser::parse(input)
    }

    #[must_use]
    pub const fn requires_admin(&self) -> bool {
        matches!(self, Self::Game(_) | Self::Entry(_) | Self::Card(_))
    }
}
