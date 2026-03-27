use guenther_core::comments::global_comments;
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
    };

    Ok(())
}
