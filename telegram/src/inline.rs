use crate::voice_lines::{VoiceLine, search_voice_lines};
use serde::Deserialize;
use serde_json::{Value, json};
use teloxide::{prelude::*, types::InlineQuery};

pub async fn answer_inline_query(bot: Bot, query: InlineQuery) -> color_eyre::Result<()> {
    let results = search_voice_lines(&query.query)
        .await?
        .into_iter()
        .map(build_inline_result)
        .collect::<Vec<_>>();

    answer_inline_query_raw(&bot, &query.id.to_string(), results).await?;

    Ok(())
}

fn build_inline_result(line: VoiceLine) -> InlineResult {
    let title = normalized_title(&line);

    InlineResult::CachedVoice {
        id: line.id,
        voice_file_id: line.file_id,
        title,
    }
}

async fn answer_inline_query_raw(
    bot: &Bot,
    inline_query_id: &str,
    results: Vec<InlineResult>,
) -> color_eyre::Result<()> {
    let payload = AnswerInlineQueryPayload {
        inline_query_id,
        results,
        cache_time: 0,
        is_personal: true,
    };
    let url = telegram_method_url(bot, "answerInlineQuery");

    let response = bot
        .client()
        .post(url)
        .json(&payload.to_json())
        .send()
        .await?;
    let body = response.text().await?;
    let telegram_response = serde_json::from_str::<TelegramResponse>(&body)?;

    if telegram_response.ok {
        return Ok(());
    }

    Err(color_eyre::eyre::eyre!(
        "Telegram inline query failed: {}; response body: {}",
        telegram_response
            .description
            .unwrap_or_else(|| "unknown error".to_owned()),
        body
    ))
}

fn telegram_method_url(bot: &Bot, method_name: &str) -> String {
    let mut url = bot.api_url();
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    {
        let mut segments = url
            .path_segments_mut()
            .expect("telegram api url must support path segments");
        segments.push(&format!("bot{}", bot.token()));
        segments.push(method_name);
    }
    url.to_string()
}

struct AnswerInlineQueryPayload<'a> {
    inline_query_id: &'a str,
    results: Vec<InlineResult>,
    cache_time: u32,
    is_personal: bool,
}

enum InlineResult {
    CachedVoice {
        id: String,
        voice_file_id: String,
        title: String,
    },
}

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    description: Option<String>,
}

impl AnswerInlineQueryPayload<'_> {
    fn to_json(&self) -> Value {
        json!({
            "inline_query_id": self.inline_query_id,
            "results": self.results.iter().map(InlineResult::to_json).collect::<Vec<_>>(),
            "cache_time": self.cache_time,
            "is_personal": self.is_personal,
        })
    }
}

impl InlineResult {
    fn to_json(&self) -> Value {
        match self {
            Self::CachedVoice {
                id,
                voice_file_id,
                title,
            } => json!({
                "type": "voice",
                "id": id,
                "voice_file_id": voice_file_id,
                "title": title,
            }),
        }
    }
}

fn normalized_title(line: &VoiceLine) -> String {
    let trimmed = line.title.trim();
    if trimmed.is_empty() {
        return line.id.clone();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::telegram_method_url;
    use teloxide::Bot;

    #[test]
    fn builds_expected_bot_api_method_url() {
        let bot = Bot::new("123:abc");

        assert_eq!(
            telegram_method_url(&bot, "answerInlineQuery"),
            "https://api.telegram.org/bot123:abc/answerInlineQuery"
        );
    }
}
