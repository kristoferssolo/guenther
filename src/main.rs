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
    handler::{Handler, create_handlers},
    inline::answer_inline_query,
    media_link::{MediaLink, extract_media_links},
    router::{RouteAction, decide_route},
    voice_lines::capture_incoming_voice_line,
};
use dotenv::dotenv;
use guenther::{
    cache::MediaCache,
    comments::{Comments, failure_comment},
    config::{Config, global_config},
    db,
    error::{Error, Result as MediaResult},
    telemetry::setup_logger,
};
use std::{future::Future, sync::Arc};
use teloxide::{dispatching::UpdateFilterExt, dptree, prelude::*, types::ChatId};
use tracing::{Span, error, info, warn};

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

    let pool = db::connect_from_env().await?;
    let media_cache = MediaCache::new(pool.clone());
    info!("database initialized");

    #[cfg(feature = "bingo")]
    let bingo_store = BingoStore::new(pool);

    #[cfg(feature = "bingo")]
    let schema = dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_callback_query().endpoint(bingo_callback_handler))
        .branch(Update::filter_inline_query().endpoint(answer_inline_query));

    #[cfg(not(feature = "bingo"))]
    let schema = dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_inline_query().endpoint(answer_inline_query));

    #[cfg_attr(not(feature = "bingo"), allow(unused_mut))]
    let mut deps = dptree::deps![handlers, bot_name, media_cache];
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
    handlers: Arc<[Handler]>,
    bot_name: Arc<str>,
    cache: MediaCache,
    #[cfg(feature = "bingo")] bingo_store: BingoStore,
    #[cfg(feature = "bingo")] admin_cache: AdminCache,
) -> color_eyre::Result<()> {
    let span = Span::current();
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
            span.record("route", "command");
            span.record("command", cmd.name());
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
        RouteAction::HandleMessage => {
            span.record("route", "message");
            process_message(&bot, &msg, &handlers, &cache).await;
        }
        RouteAction::Ignore => {
            span.record("route", "ignored");
        }
    }

    Ok(())
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

async fn process_message(bot: &Bot, msg: &Message, handlers: &[Handler], cache: &MediaCache) {
    let links = extract_media_links(msg.text(), msg.caption(), handlers);
    process_links(
        links,
        handlers,
        |handler, link| async move { handler.handle(bot, msg.chat.id, &link, cache).await },
        |link, err| async move { report_media_failure(bot, msg.chat.id, &link, &err).await },
    )
    .await;
}

async fn process_links<F, Fut, E, Eut>(
    links: Vec<MediaLink>,
    handlers: &[Handler],
    mut handle: F,
    mut on_error: E,
) where
    F: FnMut(Handler, MediaLink) -> Fut,
    Fut: Future<Output = MediaResult<()>>,
    E: FnMut(MediaLink, Error) -> Eut,
    Eut: Future<Output = ()>,
{
    for link in links {
        let Some(handler) = handlers
            .iter()
            .find(|handler| handler.platform() == link.platform)
            .cloned()
        else {
            continue;
        };

        if let Err(err) = handle(handler, link.clone()).await {
            on_error(link, err).await;
        }
    }
}

async fn report_media_failure(bot: &Bot, chat_id: ChatId, link: &MediaLink, err: &Error) {
    error!(platform = %link.platform, %err, "Media handler failed");
    let _ = bot.send_message(chat_id, failure_comment()).await;
    if let Some(admin_chat_id) = global_config().chat_id {
        let _ = bot
            .send_message(ChatId(admin_chat_id), err.to_string())
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use guenther::config::Platform;

    #[tokio::test]
    async fn failed_link_does_not_stop_later_links() {
        let handlers = assert_ok!(create_handlers(&guenther::config::PlatformConfig::default()));
        let links = extract_media_links(
            Some("https://x.com/driver/status/111 https://www.youtube.com/shorts/after-222"),
            None,
            &handlers,
        );
        let mut attempted = Vec::new();
        let mut failures = Vec::new();

        process_links(
            links,
            &handlers,
            |_, link| {
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
