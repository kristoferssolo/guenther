use crate::voice_lines::search_voice_lines;
use teloxide::{
    payloads::AnswerInlineQuerySetters,
    prelude::*,
    types::{InlineQuery, InlineQueryResult, InlineQueryResultCachedVoice},
};

pub async fn answer_inline_query(bot: Bot, query: InlineQuery) -> color_eyre::Result<()> {
    let results = search_voice_lines(&query.query)
        .await?
        .into_iter()
        .map(|line| {
            InlineQueryResult::CachedVoice(InlineQueryResultCachedVoice::new(
                line.id,
                line.file_id.into(),
                line.title,
            ))
        })
        .collect::<Vec<_>>();

    bot.answer_inline_query(query.id, results)
        .cache_time(0)
        .is_personal(true)
        .await?;

    Ok(())
}
