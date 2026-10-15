use crate::voice_lines::{VoiceLine, VoiceLines, VoiceLinesError};
use teloxide::{
    RequestError,
    prelude::*,
    types::{FileId, InlineQuery, InlineQueryResult, InlineQueryResultCachedVoice},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InlineError {
    #[error(transparent)]
    VoiceLines(#[from] VoiceLinesError),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] RequestError),
}

/// Answer an inline query with the voice lines that match its text.
///
/// # Errors
///
/// Returns an error if the voice lines cannot be searched or Telegram rejects the answer.
pub async fn answer_inline_query(
    bot: Bot,
    query: InlineQuery,
    voice_lines: &VoiceLines,
) -> Result<(), InlineError> {
    let results = voice_lines
        .search(&query.query)
        .await?
        .into_iter()
        .map(build_inline_result)
        .collect::<Vec<_>>();

    bot.answer_inline_query(query.id, results)
        .cache_time(0)
        .is_personal(true)
        .await?;

    Ok(())
}

fn build_inline_result(line: VoiceLine) -> InlineQueryResult {
    let title = normalized_title(&line);

    InlineQueryResultCachedVoice::new(line.id, FileId(line.file_id), title).into()
}

fn normalized_title(line: &VoiceLine) -> String {
    let trimmed = line.title.trim();
    if trimmed.is_empty() {
        return line.id.clone();
    }
    trimmed.to_owned()
}
