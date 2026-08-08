use crate::bingo::{
    error::{BingoError, Result},
    model::{CELL_COUNT, FREE_POSITION, ImportedCell, MAX_ENTRY_CHARS},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BingoCommand {
    Help,
    Games,
    GameCreate {
        slug: String,
        name: String,
    },
    GameState {
        slug: String,
        state: GameStateCommand,
    },
    GameDefault {
        slug: String,
    },
    GameCenter {
        slug: String,
        text: String,
    },
    Entries {
        slug: Option<String>,
    },
    Add {
        slug: Option<String>,
        text: String,
    },
    Edit {
        entry_id: i64,
        text: String,
    },
    Delete {
        entry_id: i64,
    },
    Generate {
        slug: Option<String>,
        target: Option<String>,
        replace: bool,
    },
    Get {
        slug: Option<String>,
        target: Option<String>,
    },
    Import {
        slug: String,
        target: Option<String>,
        cells: Vec<ImportedCell>,
        replace: bool,
    },
    CardSet {
        slug: String,
        target: Option<String>,
        coordinate: String,
        text: String,
    },
    Reset {
        slug: Option<String>,
        target: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStateCommand {
    Activate,
    Close,
}

impl BingoCommand {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() || input.eq_ignore_ascii_case("help") {
            return Ok(Self::Help);
        }

        let (head, rest) = split_once_whitespace(input);
        match head.to_ascii_lowercase().as_str() {
            "games" => Ok(Self::Games),
            "game" => parse_game(rest),
            "entries" => Ok(Self::Entries {
                slug: optional_word(rest),
            }),
            "add" => parse_add(rest),
            "edit" => parse_edit(rest),
            "delete" => parse_delete(rest),
            "generate" => Ok(parse_card_target(rest, false)),
            "regenerate" => Ok(parse_card_target(rest, true)),
            "get" => Ok(parse_get(rest)),
            "import" => parse_import(rest, false),
            "reimport" => parse_import(rest, true),
            "card" => parse_card(rest),
            "reset" => Ok(parse_reset(rest)),
            unknown => Err(BingoError::InvalidCommand(format!(
                "unknown bingo command `{unknown}`; use /bingo help"
            ))),
        }
    }

    #[must_use]
    pub const fn requires_admin(&self) -> bool {
        !matches!(
            self,
            Self::Help | Self::Games | Self::Entries { .. } | Self::Get { .. }
        )
    }
}

fn parse_game(input: &str) -> Result<BingoCommand> {
    let (action, rest) = split_once_whitespace(input);
    match action.to_ascii_lowercase().as_str() {
        "create" => {
            let (slug, name) = required_pair(rest, "game create <slug> <name>")?;
            Ok(BingoCommand::GameCreate { slug, name })
        }
        "activate" => Ok(BingoCommand::GameState {
            slug: required_word(rest, "game activate <slug>")?,
            state: GameStateCommand::Activate,
        }),
        "close" => Ok(BingoCommand::GameState {
            slug: required_word(rest, "game close <slug>")?,
            state: GameStateCommand::Close,
        }),
        "default" => Ok(BingoCommand::GameDefault {
            slug: required_word(rest, "game default <slug>")?,
        }),
        "center" => {
            let (slug, text) = required_pair(rest, "game center <slug> <text>")?;
            Ok(BingoCommand::GameCenter { slug, text })
        }
        _ => Err(BingoError::InvalidCommand(
            "usage: /bingo game <create|activate|close|default|center> ...".to_owned(),
        )),
    }
}

fn parse_add(input: &str) -> Result<BingoCommand> {
    let (slug, text) = if let Some((slug, text)) = input.split_once('|') {
        (
            Some(required_word(slug, "add <slug> | <entry>")?),
            text.trim(),
        )
    } else {
        (None, input.trim())
    };
    validate_text(text, "entry")?;
    Ok(BingoCommand::Add {
        slug,
        text: text.to_owned(),
    })
}

fn parse_edit(input: &str) -> Result<BingoCommand> {
    let (raw_id, text) = required_pair(input, "edit <entry_id> <text>")?;
    validate_text(&text, "entry")?;
    Ok(BingoCommand::Edit {
        entry_id: parse_id(&raw_id, "entry ID")?,
        text,
    })
}

fn parse_delete(input: &str) -> Result<BingoCommand> {
    Ok(BingoCommand::Delete {
        entry_id: parse_id(&required_word(input, "delete <entry_id>")?, "entry ID")?,
    })
}

fn parse_card_target(input: &str, replace: bool) -> BingoCommand {
    let (slug, target) = parse_slug_and_target(input);
    BingoCommand::Generate {
        slug,
        target,
        replace,
    }
}

fn parse_get(input: &str) -> BingoCommand {
    let (slug, target) = parse_slug_and_target(input);
    BingoCommand::Get { slug, target }
}

fn parse_reset(input: &str) -> BingoCommand {
    let (slug, target) = parse_slug_and_target(input);
    BingoCommand::Reset { slug, target }
}

fn parse_import(input: &str, replace: bool) -> Result<BingoCommand> {
    let mut lines = input.lines();
    let header = lines.next().unwrap_or_default();
    let (slug, target) = parse_required_slug_and_optional_target(header)?;
    let grid = lines.collect::<Vec<_>>();
    if grid.len() != 5 {
        return Err(BingoError::InvalidCommand(
            "an import must contain exactly five grid rows".to_owned(),
        ));
    }

    let mut cells = Vec::with_capacity(CELL_COUNT);
    for (row_index, row) in grid.into_iter().enumerate() {
        let columns = row.split('|').collect::<Vec<_>>();
        if columns.len() != 5 {
            return Err(BingoError::InvalidCommand(format!(
                "import row {} must contain exactly five `|`-separated cells",
                row_index + 1
            )));
        }
        for raw in columns {
            let position = cells.len();
            let raw = raw.trim();
            let (marked, text) = raw
                .strip_prefix("[x]")
                .or_else(|| raw.strip_prefix("[X]"))
                .map_or((false, raw), |text| (true, text.trim()));
            let is_free = position == FREE_POSITION;
            if is_free && text != "*" {
                return Err(BingoError::InvalidCommand(
                    "the center import cell (C3) must be `*`".to_owned(),
                ));
            }
            if !is_free {
                validate_text(text, "imported cell")?;
            }
            cells.push(ImportedCell {
                text: text.to_owned(),
                marked: marked || is_free,
                is_free,
            });
        }
    }

    Ok(BingoCommand::Import {
        slug,
        target,
        cells,
        replace,
    })
}

fn parse_card(input: &str) -> Result<BingoCommand> {
    let (action, rest) = split_once_whitespace(input);
    if !action.eq_ignore_ascii_case("set") {
        return Err(BingoError::InvalidCommand(
            "usage: /bingo card set <slug> [@user] <A1-E5> <text>".to_owned(),
        ));
    }

    let mut parts = rest.split_whitespace();
    let slug = parts
        .next()
        .ok_or_else(|| usage("card set <slug> [@user] <cell> <text>"))?;
    let next = parts
        .next()
        .ok_or_else(|| usage("card set <slug> [@user] <cell> <text>"))?;
    let (target, coordinate) =
        if next.starts_with('@') || next.chars().all(|value| value.is_ascii_digit()) {
            (
                Some(next.to_owned()),
                parts
                    .next()
                    .ok_or_else(|| usage("card set <slug> [@user] <cell> <text>"))?
                    .to_owned(),
            )
        } else {
            (None, next.to_owned())
        };
    let text = parts.collect::<Vec<_>>().join(" ");
    validate_text(&text, "cell")?;
    Ok(BingoCommand::CardSet {
        slug: slug.to_owned(),
        target,
        coordinate,
        text,
    })
}

fn parse_slug_and_target(input: &str) -> (Option<String>, Option<String>) {
    let words = input.split_whitespace().collect::<Vec<_>>();
    match words.as_slice() {
        [] => (None, None),
        [only] if only.starts_with('@') || only.chars().all(|value| value.is_ascii_digit()) => {
            (None, Some((*only).to_owned()))
        }
        [slug] => (Some((*slug).to_owned()), None),
        [slug, target, ..] => (Some((*slug).to_owned()), Some((*target).to_owned())),
    }
}

fn parse_required_slug_and_optional_target(input: &str) -> Result<(String, Option<String>)> {
    let (slug, rest) = split_once_whitespace(input);
    if slug.is_empty() {
        return Err(usage("import <slug> [@user] followed by five grid rows"));
    }
    Ok((slug.to_owned(), optional_word(rest)))
}

fn required_pair(input: &str, expected: &str) -> Result<(String, String)> {
    let (first, rest) = split_once_whitespace(input);
    if first.is_empty() || rest.is_empty() {
        return Err(usage(expected));
    }
    Ok((first.to_owned(), rest.to_owned()))
}

fn required_word(input: &str, expected: &str) -> Result<String> {
    let word = input.trim();
    if word.is_empty() || word.split_whitespace().count() != 1 {
        return Err(usage(expected));
    }
    Ok(word.to_owned())
}

fn optional_word(input: &str) -> Option<String> {
    let value = input.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_id(raw: &str, label: &str) -> Result<i64> {
    raw.parse()
        .map_err(|_| BingoError::InvalidCommand(format!("invalid {label} `{raw}`")))
}

fn validate_text(text: &str, label: &str) -> Result<()> {
    let length = text.chars().count();
    if length == 0 || length > MAX_ENTRY_CHARS {
        return Err(BingoError::InvalidCommand(format!(
            "{label} text must contain between 1 and {MAX_ENTRY_CHARS} characters"
        )));
    }
    Ok(())
}

fn usage(expected: &str) -> BingoError {
    BingoError::InvalidCommand(format!("usage: /bingo {expected}"))
}

fn split_once_whitespace(input: &str) -> (&str, &str) {
    input
        .trim()
        .split_once(char::is_whitespace)
        .map_or_else(|| (input.trim(), ""), |(head, rest)| (head, rest.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let BingoCommand::Import { cells, .. } = command else {
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
            BingoCommand::Generate {
                slug: None,
                target: Some("@driver".to_owned()),
                replace: false,
            }
        );
        assert_ok_eq!(
            BingoCommand::parse("get australian-gp @driver"),
            BingoCommand::Get {
                slug: Some("australian-gp".to_owned()),
                target: Some("@driver".to_owned()),
            }
        );
    }
}
