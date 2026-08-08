mod parser;

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

#[cfg(test)]
mod tests {
    use crate::bingo::{
        command::{BingoCommand, CardAdmin, EntryAdmin},
        model::FREE_POSITION,
    };
    use claims::{assert_err, assert_ok_eq};

    #[test]
    fn parses_entry_add_with_optional_game() {
        assert_ok_eq!(
            BingoCommand::parse("add safety car"),
            BingoCommand::Entry(EntryAdmin::Add {
                slug: None,
                text: "safety car".to_owned()
            })
        );
        assert_ok_eq!(
            BingoCommand::parse("add 2026-season | safety car"),
            BingoCommand::Entry(EntryAdmin::Add {
                slug: Some("2026-season".to_owned()),
                text: "safety car".to_owned()
            })
        );
    }

    #[test]
    fn parses_marked_import_grid() {
        let input = "import 2026-season @driver\n[x] A1 | A2 | A3 | A4 | A5\nB1 | B2 | B3 | B4 | B5\nC1 | C2 | * | C4 | C5\nD1 | D2 | D3 | D4 | D5\nE1 | E2 | E3 | E4 | E5";
        let command = BingoCommand::parse(input).expect("parse import");
        let BingoCommand::Card(CardAdmin::Import { cells, .. }) = command else {
            panic!("expected import")
        };
        assert!(cells[0].marked);
        assert!(cells[FREE_POSITION].marked);
        assert!(cells[FREE_POSITION].is_free);
    }

    #[test]
    fn rejects_malformed_import() {
        assert_err!(BingoCommand::parse("import season\nA | B"));
    }

    #[test]
    fn parses_card_targets_with_and_without_game() {
        assert_ok_eq!(
            BingoCommand::parse("generate @driver"),
            BingoCommand::Card(CardAdmin::Generate {
                slug: None,
                target: Some("@driver".to_owned()),
                replace: false,
            })
        );
        assert_ok_eq!(
            BingoCommand::parse("get australian-gp @driver"),
            BingoCommand::Get {
                slug: Some("australian-gp".to_owned()),
                target: Some("@driver".to_owned()),
            }
        );
    }

    #[test]
    fn rejects_trailing_target_words() {
        assert_err!(BingoCommand::parse("get season @driver extra"));
        assert_err!(BingoCommand::parse("generate season driver"));
    }

    #[test]
    fn validates_card_position_while_parsing() {
        assert_err!(BingoCommand::parse("card set season Z9 incident"));
        assert_ok_eq!(
            BingoCommand::parse("card set season C2 incident"),
            BingoCommand::Card(CardAdmin::Set {
                slug: "season".to_owned(),
                target: None,
                position: "C2".parse().expect("test position is valid"),
                text: "incident".to_owned(),
            })
        );
    }
}
