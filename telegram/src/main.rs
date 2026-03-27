mod commands;
mod handler;
mod inline;
mod router;
mod voice_lines;

use crate::{
    commands::answer,
    handler::{Handler, create_handlers},
    inline::answer_inline_query,
    router::{RouteAction, decide_route},
    voice_lines::capture_incoming_voice_line,
};
use dotenv::dotenv;
use guenther_core::{
    comments::Comments,
    config::{Config, FAILED_FETCH_MEDIA_MESSAGE, global_config},
    telemetry::setup_logger,
};
use std::sync::Arc;
use teloxide::{dispatching::UpdateFilterExt, dptree, prelude::*, types::ChatId};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    dotenv().ok();
    color_eyre::install().expect("color-eyre install");
    setup_logger();

    Comments::load_from_file("comments.txt")
        .await
        .unwrap_or_else(|e| {
            warn!("failed to load comments.txt: {e}; using dummy comments");
            Comments::dummy()
        })
        .init()?;

    Config::from_env().init()?;

    let bot = Bot::from_env();
    let bot_name: Arc<str> = bot.get_me().await?.username().into();

    info!(name = %bot_name, "bot starting");

    let handlers = create_handlers();
    let schema = dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_inline_query().endpoint(answer_inline_query));

    Dispatcher::builder(bot, schema)
        .dependencies(dptree::deps![handlers, bot_name])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn message_handler(
    bot: Bot,
    msg: Message,
    handlers: Arc<[Handler]>,
    bot_name: Arc<str>,
) -> color_eyre::Result<()> {
    if let Err(err) = capture_incoming_voice_line(&msg).await {
        warn!(%err, "failed to capture incoming voice line metadata");
    }

    let chat_id = msg.chat.id;
    let text = msg.text().map(str::to_owned);

    match decide_route(text.as_deref(), &bot_name) {
        RouteAction::HandleCommand(cmd) => {
            if let Err(e) = answer(&bot, chat_id, cmd).await {
                error!(%e, "failed to answer command");
            }
        }
        RouteAction::HandleMessage => process_message(&bot, &msg, &handlers).await,
        RouteAction::Ignore => {}
    }

    Ok(())
}

async fn process_message(bot: &Bot, msg: &Message, handlers: &[Handler]) {
    let Some(text) = msg.text() else {
        return;
    };

    for handler in handlers {
        if let Some(url) = handler.try_extract(text) {
            if let Err(err) = handler.handle(bot, msg.chat.id, url).await {
                error!(%err, "handler failed");
                let _ = bot
                    .send_message(msg.chat.id, FAILED_FETCH_MEDIA_MESSAGE)
                    .await;
                if let Some(chat_id) = global_config().chat_id {
                    let _ = bot.send_message(ChatId(chat_id), err.to_string()).await;
                }
            }
            return;
        }
    }
}
