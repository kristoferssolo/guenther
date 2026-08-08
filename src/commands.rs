use guenther::{
    comments::global_comments,
    config::global_config,
    f1::{ScheduleView, next_race_message},
};
use teloxide::{prelude::*, utils::command::BotCommands};

#[cfg(feature = "bingo")]
use crate::bingo::{AdminCache, BingoStore, answer_bingo};

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
    /// Manage and play F1 bingo
    #[cfg(feature = "bingo")]
    Bingo(String),
}

impl Command {
    #[inline]
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Curse => "curse",
            Self::Weekend => "weekend",
            Self::Quali => "quali",
            Self::Race => "race",
            #[cfg(feature = "bingo")]
            Self::Bingo(_) => "bingo",
        }
    }
}

pub async fn answer(
    bot: &Bot,
    message: &Message,
    cmd: Command,
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
            let comment = global_comments().build_caption();
            bot.send_message(chat_id, comment).await?
        }
        Command::Weekend => send_f1_schedule(bot, chat_id, ScheduleView::Weekend).await?,
        Command::Quali => send_f1_schedule(bot, chat_id, ScheduleView::Qualifying).await?,
        Command::Race => send_f1_schedule(bot, chat_id, ScheduleView::Race).await?,
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
) -> ResponseResult<Message> {
    let offset = global_config().f1.utc_offset;
    let message = next_race_message(view, offset)
        .await
        .unwrap_or_else(|e| format!("Failed to load F1 schedule: {e}"));

    bot.send_message(chat_id, message).await
}
