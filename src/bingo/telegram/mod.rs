mod admin;
mod callback;
mod dispatch;
mod render;

use crate::bingo::{error::Result, model::KnownUser, store::BingoStore};
use teloxide::{
    prelude::*,
    types::{Message, User},
};

pub use admin::AdminCache;
pub use callback::answer_callback;

pub async fn observe_message_users(store: &BingoStore, message: &Message) -> Result<()> {
    let chat_id = message.chat.id;
    for user in message.mentioned_users() {
        if user.is_bot {
            continue;
        }
        let known = known_user(user);
        store.observe_user(chat_id, &known).await?;
    }
    Ok(())
}

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

pub(in crate::bingo::telegram) fn known_user(user: &User) -> KnownUser {
    let display_name = user.last_name.as_ref().map_or_else(
        || user.first_name.clone(),
        |last_name| format!("{} {last_name}", user.first_name),
    );
    KnownUser {
        user_id: user.id,
        username: user.username.clone(),
        display_name,
    }
}
