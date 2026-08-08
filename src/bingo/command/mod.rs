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
    Add {
        slug: Option<String>,
        text: String,
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
    SetDescription { slug: String, description: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAdmin {
    Import { slug: String },
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
        command::{BingoCommand, CardAdmin, EntryAdmin, GameAdmin},
        model::FREE_POSITION,
    };
    use claims::{assert_err, assert_ok_eq};

    #[test]
    fn parses_entry_add_with_optional_game() {
        assert_ok_eq!(
            BingoCommand::parse("add safety car"),
            BingoCommand::Add {
                slug: None,
                text: "safety car".to_owned()
            }
        );
        assert_ok_eq!(
            BingoCommand::parse("add 2026-season | safety car"),
            BingoCommand::Add {
                slug: Some("2026-season".to_owned()),
                text: "safety car".to_owned()
            }
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

    #[test]
    fn adding_entries_does_not_require_an_administrator() {
        let add = BingoCommand::parse("add safety car").expect("parse entry addition");
        let edit = BingoCommand::parse("edit 1 red flag").expect("parse entry edit");

        assert!(!add.requires_admin());
        assert!(edit.requires_admin());
    }

    #[test]
    fn accepts_entries_up_to_128_characters() {
        assert_ok_eq!(
            BingoCommand::parse(&format!("add {}", "x".repeat(128))),
            BingoCommand::Add {
                slug: None,
                text: "x".repeat(128),
            }
        );
        assert_err!(BingoCommand::parse(&format!("add {}", "x".repeat(129))));
    }

    #[test]
    fn parses_admin_entry_file_imports() {
        let command =
            BingoCommand::parse("entries import season-2026").expect("parse entry file import");

        assert_eq!(
            command,
            BingoCommand::Entry(EntryAdmin::Import {
                slug: "season-2026".to_owned()
            })
        );
        assert!(command.requires_admin());
        assert_err!(BingoCommand::parse("entries import"));
    }

    #[test]
    fn parses_setting_and_clearing_game_descriptions() {
        assert_ok_eq!(
            BingoCommand::parse("game description season Welcome to 2026"),
            BingoCommand::Game(GameAdmin::SetDescription {
                slug: "season".to_owned(),
                description: "Welcome to 2026".to_owned(),
            })
        );
        assert_ok_eq!(
            BingoCommand::parse("game description season"),
            BingoCommand::Game(GameAdmin::SetDescription {
                slug: "season".to_owned(),
                description: String::new(),
            })
        );
        assert_err!(BingoCommand::parse("game description"));
    }
}
