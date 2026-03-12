use crate::{
    download::{DownloadResult, process_download_result},
    error::Result,
};
use regex::{Error as RegexError, Regex};
use std::{pin::Pin, sync::Arc};
use teloxide::{Bot, types::ChatId};
use tracing::info;

type DownloadFn = fn(String) -> Pin<Box<dyn Future<Output = Result<DownloadResult>> + Send>>;

#[derive(Debug, Clone)]
pub struct Handler {
    name: &'static str,
    regex: Regex,
    func: DownloadFn,
}

impl Handler {
    /// Create a new handler with a regex pattern and download function.
    ///
    /// # Errors
    ///
    /// Returns `RegexError` if the regex pattern is invalid.
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

    /// Extract a URL matching this handler's regex pattern.
    #[must_use]
    pub fn try_extract<'a>(&self, text: &'a str) -> Option<&'a str> {
        self.regex
            .captures(text)
            .and_then(|c| c.get(0).map(|m| m.as_str()))
    }

    /// Handle a URL by downloading and sending the media.
    ///
    /// # Errors
    ///
    /// Returns `Error` if download or media processing fails.
    pub async fn handle(&self, bot: &Bot, chat_id: ChatId, url: &str) -> Result<()> {
        info!(handler = %self.name(), url = %url, "handling url");
        let dr = (self.func)(url.to_owned()).await?;
        process_download_result(bot, chat_id, dr).await
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
            crate::download::platform::instagram::download_instagram
        ),
        handler!(
            "youtube",
            r"https?:\/\/(?:www\.)?youtube\.com\/shorts\/[A-Za-z0-9_-]+(?:\?[^\s]*)?",
            crate::download::platform::youtube::download_youtube
        ),
        handler!(
            "twitter",
            r"https?://(?:www\.)?(?:twitter\.com|x\.com)/([A-Za-z0-9_]+(?:/[A-Za-z0-9_]+)?)/status/(\d{1,20})",
            crate::download::platform::twitter::download_twitter
        ),
        handler!(
            "tiktok",
            r"https?://(?:www\.)?(?:vm|vt|tt|tik)\.tiktok\.com/([A-Za-z0-9_-]+)[/?#]?",
            crate::download::platform::tiktok::download_tiktok
        ),
    ]
    .into()
}
