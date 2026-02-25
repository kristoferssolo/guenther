use dotenv::dotenv;
use guenther::{
    commands::{Command, answer},
    comments::Comments,
    config::{Config, FAILED_FETCH_MEDIA_MESSAGE, global_config},
    handler::{Handler, create_handlers},
    telemetry::setup_logger,
};
use std::sync::Arc;
use teloxide::{prelude::*, respond, utils::command::BotCommands};
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

    teloxide::repl(bot.clone(), move |bot: Bot, msg: Message| {
        let handlers = Arc::clone(&handlers);
        let bot_name = Arc::clone(&bot_name);
        async move {
            process_cmd(&bot, &msg, &bot_name).await;
            process_message(&bot, &msg, &handlers).await;
            respond(())
        }
    })
    .await;

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
                    let _ = bot.send_message(chat_id, err.to_string()).await;
                }
            }
            return;
        }
    }
}

async fn process_cmd(bot: &Bot, msg: &Message, bot_name: &str) {
    if let Some(text) = msg.text()
        && let Ok(cmd) = Command::parse(text, bot_name)
        && let Err(e) = answer(bot, msg, cmd).await
    {
        error!(%e, "failed to answer command");
    }
}
