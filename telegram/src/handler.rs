use guenther_core::{
    download::{DownloadResult, collect_supported_media},
    error::{Error, Result},
    utils::MediaKind,
};
use regex::{Error as RegexError, Regex};
use std::{path::PathBuf, pin::Pin, sync::Arc};
use teloxide::{
    Bot,
    prelude::*,
    types::{ChatId, InputFile},
};
use tracing::{error, info};

type DownloadFn = fn(String) -> Pin<Box<dyn Future<Output = Result<DownloadResult>> + Send>>;

#[derive(Debug, Clone)]
pub struct Handler {
    name: &'static str,
    regex: Regex,
    func: DownloadFn,
}

impl Handler {
    pub fn new(
        name: &'static str,
        regex_pattern: &'static str,
        func: DownloadFn,
    ) -> std::result::Result<Self, RegexError> {
        let regex = Regex::new(regex_pattern)?;
        Ok(Self { name, regex, func })
    }

    #[inline]
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub fn try_extract<'a>(&self, text: &'a str) -> Option<&'a str> {
        self.regex
            .captures(text)
            .and_then(|c| c.get(0).map(|m| m.as_str()))
    }

    pub async fn handle(&self, bot: &Bot, chat_id: ChatId, url: &str) -> Result<()> {
        info!(handler = %self.name(), url = %url, "handling url");
        let dr = (self.func)(url.to_owned()).await?;
        let (_tempdir, media_items) = collect_supported_media(dr).await?;

        for (path, kind) in media_items {
            send_media_from_path(bot, chat_id, path, kind).await?;
        }

        Ok(())
    }
}

macro_rules! handler {
    ($feature:expr, $regex:expr, $download_fn:path) => {
        #[cfg(feature = $feature)]
        Handler::new($feature, $regex, |url: String| Box::pin($download_fn(url))).expect(concat!(
            "failed to create ",
            $feature,
            " handler"
        ))
    };
}

#[must_use]
pub fn create_handlers() -> Arc<[Handler]> {
    [
        handler!(
            "instagram",
            r"https?://(?:www\.)?(?:instagram\.com|instagr\.am)/(?:reel|tv)/([A-Za-z0-9_-]+)",
            guenther_core::download::platform::instagram::download_instagram
        ),
        handler!(
            "youtube",
            r"https?:\/\/(?:www\.)?youtube\.com\/shorts\/[A-Za-z0-9_-]+(?:\?[^\s]*)?",
            guenther_core::download::platform::youtube::download_youtube
        ),
        handler!(
            "twitter",
            r"https?://(?:www\.)?(?:twitter\.com|x\.com)/([A-Za-z0-9_]+(?:/[A-Za-z0-9_]+)?)/status/(\d{1,20})",
            guenther_core::download::platform::twitter::download_twitter
        ),
        handler!(
            "tiktok",
            r"https?://(?:www\.)?(?:vm|vt|tt|tik)\.tiktok\.com/([A-Za-z0-9_-]+)[/?#]?",
            guenther_core::download::platform::tiktok::download_tiktok
        ),
    ]
    .into()
}

async fn send_media_from_path(
    bot: &Bot,
    chat_id: ChatId,
    path: PathBuf,
    kind: MediaKind,
) -> Result<()> {
    let caption = guenther_core::comments::global_comments().build_caption();
    let input = InputFile::file(path);

    macro_rules! send_msg {
        ($request_expr:expr) => {{
            let mut request = $request_expr;
            request = request.caption(caption.clone());
            match request.await {
                Ok(message) => info!(message_id = message.id.to_string(), "{} sent", kind),
                Err(e) => {
                    error!("Failed to send {}: {e}", kind.to_str());
                    return Err(Error::other(format!("telegram request failed: {e}")));
                }
            }
        }};
    }

    match kind {
        MediaKind::Video => send_msg!(bot.send_video(chat_id, input)),
        MediaKind::Image => send_msg!(bot.send_photo(chat_id, input)),
        MediaKind::Unknown => {
            bot.send_message(chat_id, "No supported media found")
                .await
                .map_err(|e| Error::other(format!("telegram request failed: {e}")))?;
            return Err(Error::UnknownMediaKind);
        }
    }

    Ok(())
}
