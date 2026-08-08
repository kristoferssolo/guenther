#[cfg(feature = "bingo")]
mod bingo;
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
use guenther::{
    comments::{Comments, failure_comment},
    config::{Config, global_config},
    telemetry::setup_logger,
};
use std::sync::Arc;
use teloxide::{dispatching::UpdateFilterExt, dptree, prelude::*, types::ChatId};
use tracing::{error, info, warn};

#[cfg(feature = "bingo")]
use crate::bingo::{AdminCache, BingoStore, answer_callback, observe_message_users};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    dotenv().ok();
    color_eyre::install()?;
    setup_logger();

    Comments::load_from_file("comments.txt")
        .await
        .unwrap_or_else(|e| {
            warn!("Failed to load comments.txt: {e}; using fallback comments");
            Comments::default()
        })
        .init()?;

    Config::from_env().init()?;

    let bot = Bot::from_env();
    let bot_name: Arc<str> = bot.get_me().await?.username().into();

    info!(name = %bot_name, "bot starting");

    let handlers = create_handlers(&global_config().platforms)?;
    let enabled_platforms = handlers
        .iter()
        .map(|handler| handler.platform().to_string())
        .collect::<Vec<_>>();
    info!(?enabled_platforms, "platform handlers configured");

    #[cfg(feature = "bingo")]
    {
        let bingo_store = BingoStore::connect_from_env().await?;
        let admin_cache = AdminCache::default();
        info!("bingo database initialized");
        let schema = dptree::entry()
            .branch(Update::filter_message().endpoint(message_handler))
            .branch(Update::filter_callback_query().endpoint(bingo_callback_handler))
            .branch(Update::filter_inline_query().endpoint(answer_inline_query));

        Dispatcher::builder(bot, schema)
            .dependencies(dptree::deps![handlers, bot_name, bingo_store, admin_cache])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;
    }

    #[cfg(not(feature = "bingo"))]
    {
        let schema = dptree::entry()
            .branch(Update::filter_message().endpoint(message_handler))
            .branch(Update::filter_inline_query().endpoint(answer_inline_query));

        Dispatcher::builder(bot, schema)
            .dependencies(dptree::deps![handlers, bot_name])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;
    }

    Ok(())
}

async fn message_handler(
    bot: Bot,
    msg: Message,
    handlers: Arc<[Handler]>,
    bot_name: Arc<str>,
    #[cfg(feature = "bingo")] bingo_store: BingoStore,
    #[cfg(feature = "bingo")] admin_cache: AdminCache,
) -> color_eyre::Result<()> {
    if let Err(err) = capture_incoming_voice_line(&bot, &msg).await {
        warn!(%err, "Failed to capture incoming voice line metadata");
    }

    let text = msg.text().or_else(|| msg.caption()).map(str::to_owned);

    #[cfg(feature = "bingo")]
    if let Err(err) = observe_message_users(&bingo_store, &msg).await {
        warn!(%err, "Failed to remember Telegram user for bingo");
    }

    match decide_route(text.as_deref(), &bot_name) {
        RouteAction::HandleCommand(cmd) => {
            if let Err(e) = answer(
                &bot,
                &msg,
                cmd,
                #[cfg(feature = "bingo")]
                &bingo_store,
                #[cfg(feature = "bingo")]
                &admin_cache,
            )
            .await
            {
                error!(%e, "Failed to answer command");
            }
        }
        RouteAction::HandleMessage => process_message(&bot, &msg, &handlers).await,
        RouteAction::Ignore => {}
    }

    Ok(())
}

#[cfg(feature = "bingo")]
async fn bingo_callback_handler(
    bot: Bot,
    query: teloxide::types::CallbackQuery,
    bingo_store: BingoStore,
    admin_cache: AdminCache,
) -> color_eyre::Result<()> {
    answer_callback(&bot, &query, &bingo_store, &admin_cache).await?;
    Ok(())
}

async fn process_message(bot: &Bot, msg: &Message, handlers: &[Handler]) {
    let Some(text) = msg.text() else {
        return;
    };

    for handler in handlers {
        if let Some(url) = handler.try_extract(text) {
            if let Err(err) = handler.handle(bot, msg.chat.id, url).await {
                error!(%err, "Handler failed");
                let _ = bot.send_message(msg.chat.id, failure_comment()).await;
                if let Some(chat_id) = global_config().chat_id {
                    let _ = bot.send_message(ChatId(chat_id), err.to_string()).await;
                }
            }
            return;
        }
    }
}
