mod args;
mod parser;

use crate::bingo::{
    error::Result,
    model::{EntryId, GameState, ImportedCell, Position},
};

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
    Edit { entry_id: EntryId, text: String },
    Delete { entry_id: EntryId },
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
        entry_id: EntryId,
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
        model::{EntryId, FREE_POSITION},
    };
    use claims::{assert_err, assert_matches, assert_ok, assert_ok_eq, assert_some};

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
        let input = "import 2026-season @driver\n[x] 81 | 60 | 38 | 41 | 39\n66 | 55 | 67 | 77 | 43\n35 | 3 | * | 17 | 1\n24 | 61 | 13 | 28 | 19\n63 | 30 | 37 | 29 | 33";
        let command = assert_ok!(BingoCommand::parse(input));
        assert_matches!(&command, BingoCommand::Card(CardAdmin::Import { .. }));
        let BingoCommand::Card(CardAdmin::Import { cells, .. }) = command else {
            return;
        };
        let first = assert_some!(cells.first());
        let free = assert_some!(cells.get(FREE_POSITION));
        assert!(first.marked);
        assert_eq!(first.entry_id, Some(EntryId::from(81)));
        assert!(free.marked);
        assert!(free.is_free);
        assert_eq!(free.entry_id, None);
    }

    #[test]
    fn rejects_malformed_import() {
        assert_err!(BingoCommand::parse("import season\nA | B"));
        assert_err!(BingoCommand::parse(
            "import season\nA | 2 | 3 | 4 | 5\n6 | 7 | 8 | 9 | 10\n11 | 12 | * | 14 | 15\n16 | 17 | 18 | 19 | 20\n21 | 22 | 23 | 24 | 25"
        ));
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
        assert_err!(BingoCommand::parse("card set season Z9 42"));
        assert_ok_eq!(
            BingoCommand::parse("card set season C2 42"),
            BingoCommand::Card(CardAdmin::Set {
                slug: "season".to_owned(),
                target: None,
                position: assert_ok!("C2".parse()),
                entry_id: EntryId::from(42),
            })
        );
        assert_ok_eq!(
            BingoCommand::parse("card set season @driver C2 42"),
            BingoCommand::Card(CardAdmin::Set {
                slug: "season".to_owned(),
                target: Some("@driver".to_owned()),
                position: assert_ok!("C2".parse()),
                entry_id: EntryId::from(42),
            })
        );
        assert_err!(BingoCommand::parse("card set season C2 incident"));
        assert_err!(BingoCommand::parse("card set season C2 42 trailing"));
    }

    #[test]
    fn adding_entries_does_not_require_an_administrator() {
        let add = assert_ok!(BingoCommand::parse("add safety car"));
        let edit = assert_ok!(BingoCommand::parse("edit 1 red flag"));

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
        let command = assert_ok!(BingoCommand::parse("entries import season-2026"));

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
