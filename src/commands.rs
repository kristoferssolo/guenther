#[cfg(feature = "bingo")]
use crate::bingo::{AdminCache, BingoStore, answer_bingo};
use guenther::{
    comments::Comments,
    f1::{F1, ScheduleView},
};
use teloxide::{prelude::*, utils::command::BotCommands};

#[derive(Debug, Clone, PartialEq, Eq, BotCommands)]
#[command(rename_rule = "lowercase")]
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
    /// Manage and play F1 bingo
    #[cfg(feature = "bingo")]
    Bingo(String),
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

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Curse => "curse",
            Self::Weekend => "weekend",
            Self::Quali => "quali",
            Self::Race => "race",
            Self::Countdown => "countdown",
            Self::Standings => "standings",
            #[cfg(feature = "bingo")]
            Self::Bingo(_) => "bingo",
        }
    }
}

pub async fn answer(
    bot: &Bot,
    message: &Message,
    cmd: Command,
    comments: &Comments,
    f1: &F1,
    #[cfg(feature = "bingo")] bingo_store: &BingoStore,
    #[cfg(feature = "bingo")] admin_cache: &AdminCache,
) -> color_eyre::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_matches;

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
}
