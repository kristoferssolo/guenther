use dotenvy::dotenv;
use guenther::{
    cache::MediaCache,
    commands::{AdminContext, CommandContext, RouteAction, answer, route_message},
    comments::{Comments, failure_comment},
    config::Config,
    db,
    download::platform::Downloader,
    f1::F1,
    ffmpeg::Ffmpeg,
    handler::{HandlerError, MediaFitter, MediaHandlers, MediaLink, UploadLimits},
    inline::answer_inline_query,
    media_posts::MediaPosts,
    status::BotStatus,
    telemetry::{ErrorChain, setup_logger},
    utils::NO_LINK_PREVIEW,
    voice_lines::VoiceLines,
};
use std::{sync::Arc, time::Instant};
use teloxide::{
    dispatching::UpdateFilterExt,
    dptree,
    prelude::*,
    types::{ChatId, InlineQuery},
};
use tracing::{Instrument, Span, debug, error, error_span, info, warn};

#[cfg(any(
    feature = "bingo",
    feature = "llm-captions",
    feature = "voice-line-capture"
))]
use guenther::telegram_file::TelegramFiles;
#[cfg(feature = "llm-captions")]
use guenther::{handler::ReplyResponder, llm::Captioner};
#[cfg(feature = "llm-captions")]
use teloxide::types::Me;

#[cfg(feature = "bingo")]
use guenther::bingo::{AdminCache, BingoStore, answer_callback, observe_message_users};

#[derive(Clone)]
struct AppState {
    bot_name: Arc<str>,
    comments: Arc<Comments>,
    f1: F1,
    admin_chat_id: Option<ChatId>,
    voice_lines: VoiceLines,
    media_posts: MediaPosts,
    #[cfg(feature = "bingo")]
    telegram_files: TelegramFiles,
    /// In-character replies; `None` when `OpenRouter` is not configured.
    #[cfg(feature = "llm-captions")]
    replies: Option<ReplyResponder>,
    status: Arc<BotStatus>,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    dotenv().ok();
    color_eyre::install()?;
    setup_logger();

    let comments = Arc::new(
        Comments::load_from_file("comments.txt")
            .await
            .unwrap_or_else(|err| {
                warn!(
                    error = %ErrorChain(&err),
                    path = "comments.txt",
                    "Failed to load comments; using fallback comments"
                );
                Comments::default()
            }),
    );

    let config = Config::from_env();
    #[cfg(any(
        feature = "bingo",
        feature = "llm-captions",
        feature = "voice-line-capture"
    ))]
    let telegram_files = TelegramFiles::from_config(&config.telegram);
    let voice_lines = VoiceLines::from_env();
    #[cfg(feature = "voice-line-capture")]
    let voice_lines = voice_lines.with_telegram_files(telegram_files.clone());
    let f1 = F1::new(config.f1)?;

    let mut bot = Bot::from_env();
    if let Some(api_url) = config.telegram.api_url.clone() {
        bot = bot.set_api_url(api_url);
    }
    let upload_limits = UploadLimits::for_telegram(&config.telegram);
    let me = bot.get_me().await?;
    let bot_name: Arc<str> = me.username().into();

    info!(
        name = %bot_name,
        version = env!("CARGO_PKG_VERSION"),
        api_host = bot.api_url().host_str(),
        local_api = config.telegram.is_local(),
        upload_limit = upload_limits.file,
        "Bot starting"
    );

    let downloader = Downloader::new(config.cobalt.clone())?;
    let handlers = MediaHandlers::new(
        &config.platforms,
        downloader,
        MediaFitter::new(Ffmpeg::new(&config.ffmpeg), upload_limits),
        comments.clone(),
    )?;
    #[cfg(feature = "llm-captions")]
    let (handlers, replies) = enable_llm(handlers, &config, &comments, &telegram_files, &me)?;
    let enabled_platforms = handlers
        .enabled_platforms()
        .map(|platform| platform.to_string())
        .collect::<Vec<_>>();
    info!(?enabled_platforms, "Platform handlers configured");

    let pool = db::connect(&config.database_url).await?;
    let media_cache = MediaCache::new(pool.clone());
    let media_posts = MediaPosts::new(pool.clone());
    info!("Database ready");
    let status = Arc::new(BotStatus::new(handlers.clone(), media_cache.clone()));

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
        media_posts,
        #[cfg(feature = "bingo")]
        telegram_files,
        #[cfg(feature = "llm-captions")]
        replies,
        status,
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
    info!("Bot stopped");

    Ok(())
}

/// Attach LLM captions to `handlers` and build in-character replies when
/// `OpenRouter` is configured.
#[cfg(feature = "llm-captions")]
fn enable_llm(
    handlers: MediaHandlers,
    config: &Config,
    comments: &Arc<Comments>,
    files: &TelegramFiles,
    me: &Me,
) -> color_eyre::Result<(MediaHandlers, Option<ReplyResponder>)> {
    let Some(openrouter) = &config.openrouter else {
        info!("LLM captions and replies disabled; OpenRouter is not configured");
        return Ok((handlers, None));
    };
    info!(
        model = %openrouter.model,
        video_input = openrouter.video_input,
        reply_cooldown = ?openrouter.reply_cooldown,
        "LLM captions and replies enabled"
    );
    let captioner = Captioner::new(openrouter, Ffmpeg::new(&config.ffmpeg), comments.clone())?;
    let replies = ReplyResponder::new(
        captioner.clone(),
        files.clone(),
        me.id,
        me.username().into(),
        openrouter.reply_cooldown,
    );
    Ok((
        handlers.with_captioner(captioner, files.clone()),
        Some(replies),
    ))
}

// Context spans use the `error` level so their fields stay on warnings and
// errors under any `RUST_LOG` that keeps those events; the formatter does not
// print span levels.
#[tracing::instrument(
    name = "telegram.message",
    level = "error",
    skip_all,
    fields(
        chat_id = msg.chat.id.0,
        message_id = msg.id.0,
        user_id = msg.from.as_ref().map(|user| user.id.0),
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
        warn!(error = %ErrorChain(&err), "Failed to capture incoming voice line metadata");
    }

    let text = msg.text().or_else(|| msg.caption()).map(str::to_owned);

    #[cfg(feature = "bingo")]
    if let Err(err) = observe_message_users(&bingo_store, &msg).await {
        warn!(error = %ErrorChain(&err), "Failed to remember Telegram user for bingo");
    }

    match route_message(text.as_deref(), &state.bot_name) {
        RouteAction::HandleCommand(cmd) => {
            span.record("route", "command");
            span.record("command", <&'static str>::from(&cmd));
            let started_at = Instant::now();
            let answered = answer(
                &bot,
                &msg,
                cmd,
                CommandContext {
                    comments: &state.comments,
                    f1: &state.f1,
                    media_posts: &state.media_posts,
                    admin: AdminContext {
                        chat_id: state.admin_chat_id,
                        handlers: &handlers,
                        cache: &cache,
                        status: &state.status,
                    },
                    #[cfg(feature = "bingo")]
                    bingo_store: &bingo_store,
                    #[cfg(feature = "bingo")]
                    admin_cache: &admin_cache,
                    #[cfg(feature = "bingo")]
                    telegram_files: &state.telegram_files,
                },
            )
            .await;
            match answered {
                Ok(()) => info!(elapsed = ?started_at.elapsed(), "Handled command"),
                Err(err) => error!(error = %ErrorChain(&err), "Failed to answer command"),
            }
        }
        RouteAction::HandleMessage => {
            // Supported links take precedence over an in-character reply.
            #[cfg(feature = "llm-captions")]
            if let Some(replies) = &state.replies
                && let Some(reply) = replies.detect(&msg)
                && handlers.extract(msg.text(), msg.caption()).is_empty()
            {
                span.record("route", "reply");
                replies.answer(&bot, reply);
                return Ok(());
            }
            span.record("route", "message");
            process_message(
                &bot,
                &msg,
                &handlers,
                &cache,
                &state.media_posts,
                state.admin_chat_id,
            )
            .await;
        }
        RouteAction::Ignore => {
            span.record("route", "ignored");
        }
    }

    Ok(())
}

#[tracing::instrument(
    name = "telegram.inline_query",
    level = "error",
    skip_all,
    fields(user_id = query.from.id.0)
)]
async fn inline_query_handler(
    bot: Bot,
    query: InlineQuery,
    state: AppState,
) -> color_eyre::Result<()> {
    // Telegram sends a query per keystroke, so only failures are worth more
    // than debug.
    match answer_inline_query(bot, query, &state.voice_lines).await {
        Ok(results) => debug!(results, "Answered inline query"),
        Err(err) => warn!(error = %ErrorChain(&err), "Failed to answer inline query"),
    }
    Ok(())
}

#[cfg(feature = "bingo")]
#[tracing::instrument(
    name = "telegram.callback",
    level = "error",
    skip_all,
    fields(
        chat_id = query.regular_message().map(|message| message.chat.id.0),
        message_id = query.regular_message().map(|message| message.id.0),
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
        error!(error = %ErrorChain(&err), "Failed to answer bingo callback");
    }
    Ok(())
}

async fn process_message(
    bot: &Bot,
    msg: &Message,
    handlers: &MediaHandlers,
    cache: &MediaCache,
    media_posts: &MediaPosts,
    admin_chat_id: Option<ChatId>,
) {
    for link in handlers.extract(msg.text(), msg.caption()) {
        let span = error_span!("media", platform = %link.platform, url = %link.original_url);
        let handled = handlers
            .handle(bot, msg, &link, cache, media_posts)
            .instrument(span.clone())
            .await;
        if let Err(err) = handled {
            handlers.stats().record_failure(link.platform);
            report_media_failure(bot, msg.chat.id, admin_chat_id, &link, &err)
                .instrument(span)
                .await;
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
    // Rendered up front: the borrowed `dyn Error` is not `Send` across awaits.
    let err = ErrorChain(err).to_string();
    error!(error = %err, "Media handler failed");
    if let Err(err) = bot.send_message(chat_id, failure_comment()).await {
        warn!(error = %ErrorChain(&err), "Failed to send the failure reply");
    }
    let Some(admin_chat_id) = admin_chat_id else {
        return;
    };
    let report = format!("{} failed: {}\n{err}", link.platform, link.original_url);
    if let Err(err) = bot
        .send_message(admin_chat_id, report)
        .link_preview_options(NO_LINK_PREVIEW)
        .await
    {
        warn!(error = %ErrorChain(&err), "Failed to report the failure to the admin chat");
    }
}
