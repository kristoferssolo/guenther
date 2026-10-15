#[cfg(feature = "bingo")]
mod bingo;
mod commands;

use crate::commands::{RouteAction, answer, route_message};
use dotenvy::dotenv;
use guenther::{
    cache::MediaCache,
    comments::{Comments, failure_comment},
    config::Config,
    db,
    download::platform::Downloader,
    f1::F1,
    handler::{HandlerError, MediaHandlers, MediaLink},
    inline::answer_inline_query,
    telemetry::setup_logger,
    voice_lines::VoiceLines,
};
use std::sync::Arc;
use teloxide::{dispatching::UpdateFilterExt, dptree, prelude::*, types::ChatId};
use tracing::{Span, error, info, warn};

#[cfg(feature = "bingo")]
use crate::bingo::{AdminCache, BingoStore, answer_callback, observe_message_users};

#[derive(Clone)]
struct AppState {
    bot_name: Arc<str>,
    comments: Arc<Comments>,
    f1: F1,
    admin_chat_id: Option<ChatId>,
    voice_lines: VoiceLines,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    dotenv().ok();
    color_eyre::install()?;
    setup_logger();

    let comments = Arc::new(
        Comments::load_from_file("comments.txt")
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to load comments.txt: {e}; using fallback comments");
                Comments::default()
            }),
    );

    let config = Config::from_env();
    let voice_lines = VoiceLines::from_env();
    let f1 = F1::new(config.f1)?;

    let bot = Bot::from_env();
    let bot_name: Arc<str> = bot.get_me().await?.username().into();

    info!(name = %bot_name, "bot starting");

    let downloader = Downloader::new(config.cobalt.clone())?;
    let handlers = MediaHandlers::new(&config.platforms, downloader, comments.clone())?;
    let enabled_platforms = handlers
        .enabled_platforms()
        .map(|platform| platform.to_string())
        .collect::<Vec<_>>();
    info!(?enabled_platforms, "platform handlers configured");

    let pool = db::connect(&config.database_url).await?;
    let media_cache = MediaCache::new(pool.clone());
    info!("database initialized");

    #[cfg(feature = "bingo")]
    let bingo_store = BingoStore::new(pool);

    #[cfg(feature = "bingo")]
    let schema = dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_callback_query().endpoint(bingo_callback_handler))
        .branch(Update::filter_inline_query().endpoint(inline_query_handler));

    #[cfg(not(feature = "bingo"))]
    let schema = dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_inline_query().endpoint(inline_query_handler));

    let state = AppState {
        bot_name,
        comments,
        f1,
        admin_chat_id: config.chat_id.map(ChatId),
        voice_lines,
    };

    #[cfg_attr(not(feature = "bingo"), allow(unused_mut))]
    let mut deps = dptree::deps![handlers, state, media_cache];
    #[cfg(feature = "bingo")]
    deps.insert(bingo_store);
    #[cfg(feature = "bingo")]
    deps.insert(AdminCache::default());

    Dispatcher::builder(bot, schema)
        .dependencies(deps)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

#[tracing::instrument(
    name = "telegram.message",
    skip_all,
    fields(
        chat_id = msg.chat.id.0,
        message_id = msg.id.0,
        user_id = ?msg.from.as_ref().map(|user| user.id.0),
        route = tracing::field::Empty,
        command = tracing::field::Empty,
    )
)]
async fn message_handler(
    bot: Bot,
    msg: Message,
    handlers: MediaHandlers,
    state: AppState,
    cache: MediaCache,
    #[cfg(feature = "bingo")] bingo_store: BingoStore,
    #[cfg(feature = "bingo")] admin_cache: AdminCache,
) -> color_eyre::Result<()> {
    let span = Span::current();
    #[cfg(feature = "voice-line-capture")]
    if let Err(err) = state.voice_lines.capture(&bot, &msg).await {
        warn!(%err, "Failed to capture incoming voice line metadata");
    }

    let text = msg.text().or_else(|| msg.caption()).map(str::to_owned);

    #[cfg(feature = "bingo")]
    if let Err(err) = observe_message_users(&bingo_store, &msg).await {
        warn!(%err, "Failed to remember Telegram user for bingo");
    }

    match route_message(text.as_deref(), &state.bot_name) {
        RouteAction::HandleCommand(cmd) => {
            span.record("route", "command");
            span.record("command", <&'static str>::from(&cmd));
            if let Err(e) = answer(
                &bot,
                &msg,
                cmd,
                &state.comments,
                &state.f1,
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
        RouteAction::HandleMessage => {
            span.record("route", "message");
            process_message(&bot, &msg, &handlers, &cache, state.admin_chat_id).await;
        }
        RouteAction::Ignore => {
            span.record("route", "ignored");
        }
    }

    Ok(())
}

async fn inline_query_handler(
    bot: Bot,
    query: teloxide::types::InlineQuery,
    state: AppState,
) -> color_eyre::Result<()> {
    answer_inline_query(bot, query, &state.voice_lines)
        .await
        .map_err(Into::into)
}

#[cfg(feature = "bingo")]
#[tracing::instrument(
    name = "telegram.callback",
    skip_all,
    fields(
        chat_id = ?query.regular_message().map(|message| message.chat.id.0),
        message_id = ?query.regular_message().map(|message| message.id.0),
        user_id = query.from.id.0,
    )
)]
async fn bingo_callback_handler(
    bot: Bot,
    query: teloxide::types::CallbackQuery,
    bingo_store: BingoStore,
    admin_cache: AdminCache,
) -> color_eyre::Result<()> {
    if let Err(err) = answer_callback(&bot, &query, &bingo_store, &admin_cache).await {
        error!(%err, "Failed to answer bingo callback");
    }
    Ok(())
}

async fn process_message(
    bot: &Bot,
    msg: &Message,
    handlers: &MediaHandlers,
    cache: &MediaCache,
    admin_chat_id: Option<ChatId>,
) {
    for link in handlers.extract(msg.text(), msg.caption()) {
        if let Err(err) = handlers.handle(bot, msg.chat.id, &link, cache).await {
            report_media_failure(bot, msg.chat.id, admin_chat_id, &link, &err).await;
        }
    }
}

async fn report_media_failure(
    bot: &Bot,
    chat_id: ChatId,
    admin_chat_id: Option<ChatId>,
    link: &MediaLink,
    err: &HandlerError,
) {
    error!(platform = %link.platform, %err, "Media handler failed");
    if let Err(err) = bot.send_message(chat_id, failure_comment()).await {
        warn!(platform = %link.platform, %err, "Failed to send the failure reply");
    }
    if let Some(admin_chat_id) = admin_chat_id
        && let Err(err) = bot.send_message(admin_chat_id, err.to_string()).await
    {
        warn!(platform = %link.platform, %err, "Failed to report the failure to the admin chat");
    }
}
