use crate::bingo::{
    command::{
        BingoCommand, CardAdmin, EntryAdmin, GameAdmin,
        args::{
            is_target_token, optional_word, parse_id, parse_required_slug_and_optional_target,
            parse_slug_and_target, required_pair, required_word, split_once_whitespace, usage,
        },
    },
    error::{BingoError, Result},
    model::{
        CELL_COUNT, EntryNumber, FREE_POSITION, GRID_SIDE, GameState, ImportedCell, MAX_ENTRY_CHARS,
    },
};

pub fn parse(input: &str) -> Result<BingoCommand> {
    let input = input.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("help") {
        return Ok(BingoCommand::Help);
    }

    let (head, rest) = split_once_whitespace(input);
    match head {
        value if value.eq_ignore_ascii_case("games") => Ok(BingoCommand::Games),
        value if value.eq_ignore_ascii_case("game") => parse_game(rest),
        value if value.eq_ignore_ascii_case("entries") => parse_entries(rest),
        value if value.eq_ignore_ascii_case("add") => parse_add(rest),
        value if value.eq_ignore_ascii_case("edit") => parse_edit(rest),
        value if value.eq_ignore_ascii_case("delete") => parse_delete(rest),
        value if value.eq_ignore_ascii_case("generate") => parse_card_target(rest, false),
        value if value.eq_ignore_ascii_case("regenerate") => parse_card_target(rest, true),
        value if value.eq_ignore_ascii_case("get") => parse_get(rest),
        value if value.eq_ignore_ascii_case("import") => parse_import(rest, false),
        value if value.eq_ignore_ascii_case("reimport") => parse_import(rest, true),
        value if value.eq_ignore_ascii_case("card") => parse_card(rest),
        value if value.eq_ignore_ascii_case("reset") => parse_reset(rest),
        unknown => Err(BingoError::InvalidCommand(format!(
            "Unknown bingo command `{unknown}`; use /bingo help"
        ))),
    }
}

fn parse_entries(input: &str) -> Result<BingoCommand> {
    let (action, rest) = split_once_whitespace(input);
    if action.eq_ignore_ascii_case("import") {
        return Ok(BingoCommand::Entry(EntryAdmin::Import {
            slug: required_word(rest, "entries import <game>")?.to_owned(),
        }));
    }
    Ok(BingoCommand::Entries {
        slug: optional_word(input).map(str::to_owned),
    })
}

fn parse_game(input: &str) -> Result<BingoCommand> {
    let (action, rest) = split_once_whitespace(input);
    match action {
        value if value.eq_ignore_ascii_case("create") => {
            let (slug, name) = required_pair(rest, "game create <slug> <name>")?;
            Ok(BingoCommand::Game(GameAdmin::Create {
                slug: slug.to_owned(),
                name: name.to_owned(),
            }))
        }
        value if value.eq_ignore_ascii_case("delete") => {
            Ok(BingoCommand::Game(GameAdmin::Delete {
                slug: required_word(rest, "game delete <slug>")?.to_owned(),
            }))
        }
        value if value.eq_ignore_ascii_case("activate") => {
            Ok(BingoCommand::Game(GameAdmin::SetState {
                slug: required_word(rest, "game activate <slug>")?.to_owned(),
                state: GameState::Active,
            }))
        }
        value if value.eq_ignore_ascii_case("close") => {
            Ok(BingoCommand::Game(GameAdmin::SetState {
                slug: required_word(rest, "game close <slug>")?.to_owned(),
                state: GameState::Closed,
            }))
        }
        value if value.eq_ignore_ascii_case("default") => {
            Ok(BingoCommand::Game(GameAdmin::SetDefault {
                slug: required_word(rest, "game default <slug>")?.to_owned(),
            }))
        }
        value if value.eq_ignore_ascii_case("center") => {
            let (slug, text) = required_pair(rest, "game center <slug> <text>")?;
            Ok(BingoCommand::Game(GameAdmin::SetCenter {
                slug: slug.to_owned(),
                text: text.to_owned(),
            }))
        }
        value if value.eq_ignore_ascii_case("description") => {
            let (slug, description) = split_once_whitespace(rest);
            if slug.is_empty() {
                return Err(usage("game description <slug> [text]"));
            }
            Ok(BingoCommand::Game(GameAdmin::SetDescription {
                slug: slug.to_owned(),
                description: description.to_owned(),
            }))
        }
        _ => Err(BingoError::InvalidCommand(
            "Usage: /bingo game <create|delete|activate|close|default|center|description> ..."
                .to_owned(),
        )),
    }
}

fn parse_add(input: &str) -> Result<BingoCommand> {
    let (slug, text) = if let Some((slug, text)) = input.split_once('|') {
        (
            Some(required_word(slug, "add <slug> | <entry>")?.to_owned()),
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
    const USAGE: &str = "edit [game] <entry_number> <text>";
    let (first, rest) = required_pair(input, USAGE)?;
    let (slug, raw_number, text) = if first.chars().all(|character| character.is_ascii_digit()) {
        (None, first, rest)
    } else {
        let (raw_number, text) = required_pair(rest, USAGE)?;
        (Some(first.to_owned()), raw_number, text)
    };
    validate_text(text, "entry")?;
    Ok(BingoCommand::Entry(EntryAdmin::Edit {
        slug,
        entry_number: parse_id(raw_number, "entry number")?,
        text: text.to_owned(),
    }))
}

fn parse_delete(input: &str) -> Result<BingoCommand> {
    const USAGE: &str = "delete [game] <entry_number>";
    let (first, rest) = split_once_whitespace(input);
    let (slug, raw_number) = if rest.is_empty() {
        (None, required_word(first, USAGE)?)
    } else {
        (Some(first.to_owned()), required_word(rest, USAGE)?)
    };
    Ok(BingoCommand::Entry(EntryAdmin::Delete {
        slug,
        entry_number: parse_id(raw_number, "entry number")?,
    }))
}

fn parse_card_target(input: &str, replace: bool) -> Result<BingoCommand> {
    let (slug, target) = parse_slug_and_target(input, "generate [game] [@user]")?;
    Ok(BingoCommand::Card(CardAdmin::Generate {
        slug,
        target,
        replace,
    }))
}

fn parse_get(input: &str) -> Result<BingoCommand> {
    let (slug, target) = parse_slug_and_target(input, "get [game] [@user]")?;
    Ok(BingoCommand::Get { slug, target })
}

fn parse_reset(input: &str) -> Result<BingoCommand> {
    let (slug, target) = parse_slug_and_target(input, "reset [game] [@user]")?;
    Ok(BingoCommand::Card(CardAdmin::Reset { slug, target }))
}

fn parse_import(input: &str, replace: bool) -> Result<BingoCommand> {
    let mut lines = input.lines();
    let header = lines.next().unwrap_or_default();
    let (slug, target) = parse_required_slug_and_optional_target(header)?;
    let grid = lines.collect::<Vec<_>>();
    if grid.len() != GRID_SIDE {
        return Err(BingoError::InvalidCommand(format!(
            "A card import must contain exactly {GRID_SIDE} grid rows"
        )));
    }

    let mut cells = Vec::with_capacity(CELL_COUNT);
    for (row_index, row) in grid.into_iter().enumerate() {
        let columns = row.split('|').collect::<Vec<_>>();
        if columns.len() != GRID_SIDE {
            return Err(BingoError::InvalidCommand(format!(
                "Import row {} must contain exactly {GRID_SIDE} `|`-separated cells",
                row_index.saturating_add(1)
            )));
        }
        for raw in columns {
            let position = cells.len();
            let raw = raw.trim();
            let (marked, value) = raw
                .strip_prefix("[x]")
                .or_else(|| raw.strip_prefix("[X]"))
                .map_or((false, raw), |value| (true, value.trim()));
            let is_free = position == FREE_POSITION;
            if is_free && value != "*" {
                return Err(BingoError::InvalidCommand(
                    "The center import cell (C3) must be `*`".to_owned(),
                ));
            }
            cells.push(ImportedCell {
                entry_number: (!is_free)
                    .then(|| parse_id::<EntryNumber>(value, "entry number"))
                    .transpose()?,
                marked: marked || is_free,
                is_free,
            });
        }
    }

    Ok(BingoCommand::Card(CardAdmin::Import {
        slug,
        target,
        cells,
        replace,
    }))
}

fn parse_card(input: &str) -> Result<BingoCommand> {
    const USAGE: &str = "card set <slug> [@user] <cell> <entry_number>";

    let (action, rest) = split_once_whitespace(input);
    if !action.eq_ignore_ascii_case("set") {
        return Err(BingoError::InvalidCommand(
            "Usage: /bingo card set <slug> [@user] <A1-E5> <entry_number>".to_owned(),
        ));
    }

    let mut parts = rest.split_whitespace();
    let slug = parts.next().ok_or_else(|| usage(USAGE))?;
    let next = parts.next().ok_or_else(|| usage(USAGE))?;
    let (target, coordinate) = if is_target_token(next) {
        (
            Some(next.to_owned()),
            parts.next().ok_or_else(|| usage(USAGE))?,
        )
    } else {
        (None, next)
    };
    let position = coordinate.parse()?;
    let raw_entry_number = parts.next().ok_or_else(|| usage(USAGE))?;
    if parts.next().is_some() {
        return Err(usage(USAGE));
    }
    Ok(BingoCommand::Card(CardAdmin::Set {
        slug: slug.to_owned(),
        target,
        position,
        entry_number: parse_id(raw_entry_number, "entry number")?,
    }))
}

fn validate_text(text: &str, label: &str) -> Result<()> {
    let length = text.chars().count();
    if length == 0 || length > MAX_ENTRY_CHARS {
        return Err(BingoError::InvalidCommand(format!(
            "The {label} text must contain between 1 and {MAX_ENTRY_CHARS} characters"
        )));
    }
    Ok(())
}
