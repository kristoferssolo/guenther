use guenther::{
    comments::{TELEGRAM_CAPTION_LIMIT, global_comments},
    config::{Platform, PlatformConfig},
    download::{DownloadResult, collect_supported_media},
    error::{Error, Result},
    utils::MediaKind,
};
use regex::{Error as RegexError, Regex};
use std::{path::PathBuf, pin::Pin, result::Result as StdResult, sync::Arc};
use teloxide::{
    Bot,
    prelude::*,
    types::{ChatId, InputFile},
};
use tracing::{error, info};

type DownloadFn = fn(String) -> Pin<Box<dyn Future<Output = Result<DownloadResult>> + Send>>;

#[derive(Debug, Clone)]
pub struct Handler {
    platform: Platform,
    regex: Regex,
    func: DownloadFn,
}

impl Handler {
    pub fn new(
        platform: Platform,
        regex_pattern: &'static str,
        func: DownloadFn,
    ) -> StdResult<Self, RegexError> {
        let regex = Regex::new(regex_pattern)?;
        Ok(Self {
            platform,
            regex,
            func,
        })
    }

    #[inline]
    #[must_use]
    pub const fn platform(&self) -> Platform {
        self.platform
    }

    #[must_use]
    pub fn try_extract<'a>(&self, text: &'a str) -> Option<&'a str> {
        self.regex
            .captures(text)
            .and_then(|c| c.get(0).map(|m| m.as_str()))
    }

    pub async fn handle(&self, bot: &Bot, chat_id: ChatId, url: &str) -> Result<()> {
        info!(handler = %self.platform, url = %url, "handling url");
        let dr = (self.func)(url.to_owned()).await?;
        let source_text = dr.source_text.clone();
        let (_tempdir, media_items) = collect_supported_media(dr).await?;
        let base_caption = global_comments().build_caption();
        let include_source_text =
            should_include_source_text(self.platform(), &media_items, source_text.as_deref());

        for (index, (path, kind)) in media_items.into_iter().enumerate() {
            let caption = if include_source_text && index == 0 {
                compose_caption(&base_caption, source_text.as_deref())
            } else {
                base_caption.clone()
            };
            send_media_from_path(bot, chat_id, path, kind, &caption).await?;
        }

        Ok(())
    }
}

macro_rules! handler {
    ($platform:expr, $regex:expr, $download_fn:path) => {
        Handler::new($platform, $regex, |url: String| Box::pin($download_fn(url)))
    };
}

pub fn create_handlers(platforms: &PlatformConfig) -> StdResult<Arc<[Handler]>, RegexError> {
    let handlers = [
        handler!(
            Platform::Instagram,
            r"https?://(?:www\.)?(?:instagram\.com|instagr\.am)/(?:reel|tv)/([A-Za-z0-9_-]+)",
            guenther::download::platform::instagram::download_instagram
        ),
        handler!(
            Platform::Youtube,
            r"https?://(?:www\.)?youtube\.com\/shorts\/[A-Za-z0-9_-]+(?:\?[^\s]*)?",
            guenther::download::platform::youtube::download_youtube
        ),
        handler!(
            Platform::Twitter,
            r"https?://(?:www\.)?(?:twitter\.com|x\.com)/([A-Za-z0-9_]+(?:/[A-Za-z0-9_]+)?)/status/(\d{1,20})",
            guenther::download::platform::twitter::download_twitter
        ),
        handler!(
            Platform::Tiktok,
            r"https?://(?:www\.)?(?:vm|vt|tt|tik)\.tiktok\.com/([A-Za-z0-9_-]+)[/?#]?",
            guenther::download::platform::tiktok::download_tiktok
        ),
    ]
    .into_iter()
    .collect::<StdResult<Vec<_>, _>>()?;
    Ok(handlers
        .into_iter()
        .filter(|handler| platforms.is_enabled(handler.platform()))
        .collect::<Vec<_>>()
        .into())
}

async fn send_media_from_path(
    bot: &Bot,
    chat_id: ChatId,
    path: PathBuf,
    kind: MediaKind,
    caption: &str,
) -> Result<()> {
    let input = InputFile::file(path);

    macro_rules! send_msg {
        ($request_expr:expr) => {{
            let mut request = $request_expr;
            request = request.caption(caption.to_owned());
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

fn should_include_source_text(
    platform: Platform,
    media_items: &[(PathBuf, MediaKind)],
    source_text: Option<&str>,
) -> bool {
    matches!(platform, Platform::Twitter)
        && source_text.is_some()
        && media_items
            .iter()
            .all(|(_, kind)| matches!(kind, MediaKind::Image))
}

fn compose_caption(quote: &str, source_text: Option<&str>) -> String {
    let Some(source_text) = source_text.map(str::trim).filter(|text| !text.is_empty()) else {
        return quote.to_owned();
    };

    let combined = format!("{quote}\n\n{source_text}");
    if combined.chars().count() <= TELEGRAM_CAPTION_LIMIT {
        return combined;
    }

    let reserved = quote.chars().count().saturating_add(2);
    if reserved >= TELEGRAM_CAPTION_LIMIT {
        return quote.to_owned();
    }

    let available = TELEGRAM_CAPTION_LIMIT.saturating_sub(reserved);
    let truncated = truncate_with_ellipsis(source_text, available);
    format!("{quote}\n\n{truncated}")
}

fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let truncated = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_caption_appends_source_text() {
        let caption = compose_caption("quote", Some("tweet text"));
        assert_eq!(caption, "quote\n\ntweet text");
    }

    #[test]
    fn compose_caption_ignores_missing_source_text() {
        let caption = compose_caption("quote", None);
        assert_eq!(caption, "quote");
    }
}
