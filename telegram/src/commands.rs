use guenther_core::{
    comments::global_comments,
    config::global_config,
    f1::{ScheduleView, next_race_message},
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
}

pub async fn answer(bot: &Bot, chat_id: ChatId, cmd: Command) -> ResponseResult<()> {
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
