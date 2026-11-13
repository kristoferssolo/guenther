#[cfg(feature = "bingo")]
use crate::bingo::{AdminCache, BingoError, BingoStore, answer_bingo};
use crate::{
    comments::Comments,
    f1::{F1, ScheduleView},
    media_posts::{MediaPosts, StatsPeriod},
};
use chrono::Utc;
use strum::IntoStaticStr;
use teloxide::{
    RequestError,
    prelude::*,
    types::LinkPreviewOptions,
    utils::command::{BotCommands, ParseError},
};
use thiserror::Error;
use tracing::warn;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("Telegram request failed: {0}")]
    Telegram(#[from] RequestError),

    #[cfg(feature = "bingo")]
    #[error(transparent)]
    Bingo(#[from] BingoError),
}

#[derive(Debug, Clone, PartialEq, Eq, BotCommands, IntoStaticStr)]
#[command(rename_rule = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Command {
    /// Display this text.
    #[command(aliases = ["h", "?"])]
    Help,
    /// Send a random comment
    #[command()]
    Curse,
    /// Show the next F1 weekend schedule
    #[command(aliases = ["f1"])]
    Weekend,
    /// Show the next F1 qualifying schedule
    #[command()]
    Quali,
    /// Show the next F1 race schedule
    #[command()]
    Race,
    /// Show time until the next F1 session
    #[command()]
    Countdown,
    /// Show the current F1 driver and constructor standings
    #[command()]
    Standings,
    /// Show link stats for this chat: /stats [week|month|year|all]
    #[command(parse_with = whole_argument)]
    Stats(String),
    /// Manage and play F1 bingo
    #[cfg(feature = "bingo")]
    #[command(parse_with = whole_argument)]
    Bingo(String),
}

/// Pass the whole argument text through, like the default parser.
///
/// The default parser calls `String::from_str` and converts its `Infallible`
/// error, which Rust 1.100 reports as `unreachable_code` inside the
/// `BotCommands` derive.
#[expect(
    clippy::unnecessary_wraps,
    reason = "BotCommands requires parsers to return a Result"
)]
const fn whole_argument(input: String) -> Result<(String,), ParseError> {
    Ok((input,))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAction {
    HandleCommand(Command),
    HandleMessage,
    Ignore,
}

pub fn route_message(text: Option<&str>, bot_name: &str) -> RouteAction {
    let Some(text) = text else {
        return RouteAction::Ignore;
    };
    Command::parse(text, bot_name)
        .map_or_else(|_| RouteAction::HandleMessage, RouteAction::HandleCommand)
}

/// Reply to a parsed bot command.
///
/// # Errors
///
/// Returns an error if a Telegram request fails or a bingo command fails.
#[cfg_attr(
    feature = "bingo",
    expect(
        clippy::too_many_arguments,
        reason = "commands borrow their dependencies from the dispatcher"
    )
)]
pub async fn answer(
    bot: &Bot,
    message: &Message,
    cmd: Command,
    comments: &Comments,
    f1: &F1,
    media_posts: &MediaPosts,
    #[cfg(feature = "bingo")] bingo_store: &BingoStore,
    #[cfg(feature = "bingo")] admin_cache: &AdminCache,
) -> Result<(), CommandError> {
    let chat_id = message.chat.id;
    match cmd {
        Command::Help => {
            bot.send_message(chat_id, Command::descriptions().to_string())
                .await?
        }
        Command::Curse => {
            let comment = comments.build_caption();
            bot.send_message(chat_id, comment).await?
        }
        Command::Weekend => send_f1_schedule(bot, chat_id, ScheduleView::Weekend, f1).await?,
        Command::Quali => send_f1_schedule(bot, chat_id, ScheduleView::Qualifying, f1).await?,
        Command::Race => send_f1_schedule(bot, chat_id, ScheduleView::Race, f1).await?,
        Command::Countdown => send_f1_countdown(bot, chat_id, f1).await?,
        Command::Standings => send_f1_standings(bot, chat_id, f1).await?,
        Command::Stats(input) => send_link_stats(bot, chat_id, media_posts, &input).await?,
        #[cfg(feature = "bingo")]
        Command::Bingo(input) => {
            answer_bingo(bot, message, bingo_store, admin_cache, &input).await?;
            return Ok(());
        }
    };

    Ok(())
}

async fn send_f1_schedule(
    bot: &Bot,
    chat_id: ChatId,
    view: ScheduleView,
    f1: &F1,
) -> ResponseResult<Message> {
    let message = f1
        .schedule(view)
        .await
        .unwrap_or_else(|e| format!("Failed to load F1 schedule: {e}"));

    bot.send_message(chat_id, message).await
}

async fn send_f1_countdown(bot: &Bot, chat_id: ChatId, f1: &F1) -> ResponseResult<Message> {
    let message = f1
        .countdown()
        .await
        .unwrap_or_else(|e| format!("Failed to load F1 schedule: {e}"));

    bot.send_message(chat_id, message).await
}

async fn send_f1_standings(bot: &Bot, chat_id: ChatId, f1: &F1) -> ResponseResult<Message> {
    let message = f1
        .standings()
        .await
        .unwrap_or_else(|e| format!("Failed to load F1 standings: {e}"));

    bot.send_message(chat_id, message).await
}

async fn send_link_stats(
    bot: &Bot,
    chat_id: ChatId,
    media_posts: &MediaPosts,
    input: &str,
) -> ResponseResult<Message> {
    let message = match StatsPeriod::from_argument(input) {
        Ok(period) => match media_posts.stats(chat_id, period, Utc::now()).await {
            Ok(stats) => stats.to_string(),
            Err(err) => {
                warn!(%err, "Failed to load link stats");
                format!("Failed to load link stats: {err}")
            }
        },
        Err(_) => "Usage: /stats [week|month|year|all]".to_owned(),
    };

    bot.send_message(chat_id, message)
        .link_preview_options(LinkPreviewOptions {
            is_disabled: true,
            url: None,
            prefer_small_media: false,
            prefer_large_media: false,
            show_above_text: false,
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[test]
    fn routes_commands_plain_messages_and_missing_text() {
        assert_matches!(
            route_message(Some("/help"), "guenther_bot"),
            RouteAction::HandleCommand(Command::Help)
        );
        assert_eq!(
            route_message(Some("hello"), "guenther_bot"),
            RouteAction::HandleMessage
        );
        assert_eq!(route_message(None, "guenther_bot"), RouteAction::Ignore);
    }

    #[test]
    fn stats_takes_an_optional_period() {
        assert_eq!(
            route_message(Some("/stats month"), "guenther_bot"),
            RouteAction::HandleCommand(Command::Stats("month".to_owned()))
        );
        assert_eq!(
            route_message(Some("/stats"), "guenther_bot"),
            RouteAction::HandleCommand(Command::Stats(String::new()))
        );
    }

    #[cfg(feature = "bingo")]
    #[test]
    fn bingo_takes_the_whole_argument_text() {
        assert_eq!(
            route_message(Some("/bingo mark 12 Safety car"), "guenther_bot"),
            RouteAction::HandleCommand(Command::Bingo("mark 12 Safety car".to_owned()))
        );
        assert_eq!(
            route_message(Some("/bingo"), "guenther_bot"),
            RouteAction::HandleCommand(Command::Bingo(String::new()))
        );
    }
}
