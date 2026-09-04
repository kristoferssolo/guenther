#[cfg(feature = "bingo")]
mod bingo;
mod commands;
mod handler;
mod inline;
mod media_link;
mod router;
mod voice_lines;

use crate::{
    commands::answer,
    handler::MediaHandlers,
    inline::answer_inline_query,
    media_link::MediaLink,
    router::{RouteAction, decide_route},
    voice_lines::VoiceLines,
};
use dotenv::dotenv;
use guenther::{
    cache::MediaCache,
    comments::{Comments, failure_comment},
    config::{Config, F1Config},
    db,
    download::platform::Downloader,
    error::{Error, Result as MediaResult},
    telemetry::setup_logger,
};
use std::{future::Future, sync::Arc};
use teloxide::{dispatching::UpdateFilterExt, dptree, prelude::*, types::ChatId};
use tracing::{Span, error, info, warn};

#[cfg(feature = "bingo")]
use crate::bingo::{AdminCache, BingoStore, answer_callback, observe_message_users};

#[derive(Clone)]
struct AppState {
    bot_name: Arc<str>,
    comments: Arc<Comments>,
    f1: F1Config,
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

    let pool = db::connect_from_env().await?;
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
        f1: config.f1,
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

    match decide_route(text.as_deref(), &state.bot_name) {
        RouteAction::HandleCommand(cmd) => {
            span.record("route", "command");
            span.record("command", cmd.name());
            if let Err(e) = answer(
                &bot,
                &msg,
                cmd,
                &state.comments,
                state.f1,
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
    answer_inline_query(bot, query, &state.voice_lines).await
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
        return Err(err.into());
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
    let links = handlers.extract(msg.text(), msg.caption());
    process_links(
        links,
        |link| async move { handlers.handle(bot, msg.chat.id, &link, cache).await },
        |link, err| async move {
            report_media_failure(bot, msg.chat.id, admin_chat_id, &link, &err).await;
        },
    )
    .await;
}

async fn process_links<F, Fut, E, Eut>(links: Vec<MediaLink>, mut handle: F, mut on_error: E)
where
    F: FnMut(MediaLink) -> Fut,
    Fut: Future<Output = MediaResult<()>>,
    E: FnMut(MediaLink, Error) -> Eut,
    Eut: Future<Output = ()>,
{
    for link in links {
        if let Err(err) = handle(link.clone()).await {
            on_error(link, err).await;
        }
    }
}

async fn report_media_failure(
    bot: &Bot,
    chat_id: ChatId,
    admin_chat_id: Option<ChatId>,
    link: &MediaLink,
    err: &Error,
) {
    error!(platform = %link.platform, %err, "Media handler failed");
    let _ = bot.send_message(chat_id, failure_comment()).await;
    if let Some(admin_chat_id) = admin_chat_id {
        let _ = bot.send_message(admin_chat_id, err.to_string()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use guenther::config::Platform;

    #[tokio::test]
    async fn failed_link_does_not_stop_later_links() {
        let downloader = assert_ok!(Downloader::new(guenther::config::CobaltConfig::default()));
        let handlers = assert_ok!(MediaHandlers::new(
            &guenther::config::PlatformConfig::default(),
            downloader,
            Arc::new(Comments::default()),
        ));
        let links = handlers.extract(
            Some("https://x.com/driver/status/111 https://www.youtube.com/shorts/after-222"),
            None,
        );
        let mut attempted = Vec::new();
        let mut failures = Vec::new();

        process_links(
            links,
            |link| {
                let url = link.original_url.clone();
                let failed = link.platform == Platform::Twitter;
                attempted.push(url);
                async move {
                    if failed {
                        Err(Error::other("network failure"))
                    } else {
                        Ok(())
                    }
                }
            },
            |link, _| {
                failures.push(link.original_url);
                async {}
            },
        )
        .await;

        assert_eq!(attempted.len(), 2);
        assert_eq!(failures.len(), 1);
        assert_eq!(
            attempted.get(1).map(String::as_str),
            Some("https://www.youtube.com/shorts/after-222")
        );
    }
}
