mod admin;
mod callback;
mod dispatch;
mod document;
mod render;

use crate::bingo::{error::Result, model::KnownUser, store::BingoStore};
use teloxide::{prelude::*, types::Message};

pub use crate::bingo::telegram::admin::AdminCache;
pub use crate::bingo::telegram::callback::answer_callback;

/// Remember the non-bot users a message mentions, so bingo commands can resolve them later.
///
/// # Errors
///
/// Returns an error if a user cannot be stored.
pub async fn observe_message_users(store: &BingoStore, message: &Message) -> Result<()> {
    let chat_id = message.chat.id;
    for user in message.mentioned_users() {
        if user.is_bot {
            continue;
        }
        let known = KnownUser::from(user);
        store.observe_user(chat_id, &known).await?;
    }
    Ok(())
}

/// Run a `/bingo` command, replying with user-facing errors instead of returning them.
///
/// # Errors
///
/// Returns an error if the command fails for a reason the user cannot fix, or if sending the
/// error reply fails.
pub async fn answer_bingo(
    bot: &Bot,
    message: &Message,
    store: &BingoStore,
    admin_cache: &AdminCache,
    input: &str,
) -> Result<()> {
    let result = dispatch::execute_bingo(bot, message, store, admin_cache, input).await;
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.is_user_facing() => {
            bot.send_message(message.chat.id, format!("Bingo: {error}"))
                .await?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}
