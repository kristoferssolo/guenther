use crate::bingo::error::{BingoError, Result};
use teloxide::{
    net::Download,
    prelude::{Bot, Requester},
    types::Message,
};

const MAX_ENTRY_FILE_BYTES: u32 = 64 * 1_024;
const MAX_ENTRY_FILE_LINES: usize = 1_000;

pub async fn read_entry_file(bot: &Bot, message: &Message) -> Result<Vec<String>> {
    let document = message.document().ok_or_else(|| {
        BingoError::InvalidCommand(
            "Attach a UTF-8 text file and set its caption to `/bingo entries import <game>`"
                .to_owned(),
        )
    })?;
    if document.file.size > MAX_ENTRY_FILE_BYTES {
        return Err(BingoError::InvalidCommand(format!(
            "Entry files must be no larger than {MAX_ENTRY_FILE_BYTES} bytes"
        )));
    }

    let file = bot.get_file(document.file.id.clone()).await?;
    let capacity = usize::try_from(document.file.size)
        .unwrap_or_else(|_| usize::try_from(MAX_ENTRY_FILE_BYTES).unwrap_or_default());
    let mut bytes = Vec::with_capacity(capacity);
    bot.download_file(&file.path, &mut bytes).await?;
    let text = String::from_utf8(bytes).map_err(|_| {
        BingoError::InvalidCommand("Entry files must use UTF-8 text encoding".to_owned())
    })?;
    parse_entry_lines(&text)
}

fn parse_entry_lines(text: &str) -> Result<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let entries = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Err(BingoError::InvalidCommand(
            "The entry file does not contain any entries".to_owned(),
        ));
    }
    if entries.len() > MAX_ENTRY_FILE_LINES {
        return Err(BingoError::InvalidCommand(format!(
            "Entry files may contain at most {MAX_ENTRY_FILE_LINES} non-empty lines"
        )));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok_eq};

    #[test]
    fn parses_one_trimmed_entry_per_nonempty_line() {
        assert_ok_eq!(
            parse_entry_lines("\u{feff} Safety car \n\nWet race\r\n"),
            vec!["Safety car".to_owned(), "Wet race".to_owned()]
        );
    }

    #[test]
    fn rejects_empty_entry_files() {
        assert_err!(parse_entry_lines("\n \r\n"));
    }
}
