#[cfg(feature = "llm-captions")]
mod caption;
mod delivery;
mod link;

use crate::{
    cache::MediaCache,
    comments::Comments,
    config::{Platform, PlatformConfig},
    download::{DownloadError, platform::Downloader},
    handler::{
        delivery::{compose_caption, send_media, should_include_source_text},
        link::{extract_media_links, normalize_cache_key},
    },
    media_posts::MediaPosts,
};
#[cfg(feature = "llm-captions")]
use crate::{
    handler::caption::{CaptionJob, InitialCaption, JobMedia},
    llm::Captioner,
};
use regex::{Error as RegexError, Regex};
use std::{result::Result as StdResult, sync::Arc, time::Instant};
use teloxide::{
    Bot, RequestError,
    types::{FileId, InputFile, Message},
};
use thiserror::Error;
use tracing::{debug, info, warn};

pub use crate::handler::link::MediaLink;

#[derive(Debug, Error)]
pub enum HandlerError {
    #[error(transparent)]
    Download(#[from] DownloadError),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] RequestError),
}

type Result<T> = StdResult<T, HandlerError>;

#[derive(Debug, Clone)]
struct Handler {
    platform: Platform,
    regex: Regex,
}

/// Matches supported URLs and dispatches them to the configured downloader.
#[derive(Debug, Clone)]
pub struct MediaHandlers {
    handlers: Arc<[Handler]>,
    downloader: Downloader,
    comments: Arc<Comments>,
    #[cfg(feature = "llm-captions")]
    captioner: Option<Captioner>,
}

impl MediaHandlers {
    /// Build URL matchers for the enabled platforms.
    ///
    /// # Errors
    ///
    /// Returns an error if a platform URL pattern fails to compile.
    pub fn new(
        platforms: &PlatformConfig,
        downloader: Downloader,
        comments: Arc<Comments>,
    ) -> StdResult<Self, RegexError> {
        let handlers = create_handlers(platforms)?;
        Ok(Self {
            handlers,
            downloader,
            comments,
            #[cfg(feature = "llm-captions")]
            captioner: None,
        })
    }

    /// Caption media with LLM-written lines, waiting a bounded time before
    /// falling back to a static caption that is replaced later.
    #[cfg(feature = "llm-captions")]
    #[must_use]
    pub fn with_captioner(self, captioner: Captioner) -> Self {
        Self {
            captioner: Some(captioner),
            ..self
        }
    }

    pub fn enabled_platforms(&self) -> impl Iterator<Item = Platform> + '_ {
        self.handlers.iter().map(Handler::platform)
    }

    pub fn extract(&self, text: Option<&str>, caption: Option<&str>) -> Vec<MediaLink> {
        extract_media_links(text, caption, &self.handlers)
    }

    /// Reply in the chat of `msg` with the media behind `link`, preferring
    /// cached Telegram file ids, and record the delivery in `posts`.
    ///
    /// # Errors
    ///
    /// Returns an error if the download fails or Telegram rejects the downloaded media.
    pub async fn handle(
        &self,
        bot: &Bot,
        msg: &Message,
        link: &MediaLink,
        cache: &MediaCache,
        posts: &MediaPosts,
    ) -> Result<()> {
        let cache_hit = self.deliver(bot, msg, link, cache).await?;
        if let Err(err) = posts.record_delivery(msg, link, cache_hit).await {
            warn!(platform = %link.platform, %err, "Failed to record the media post");
        }
        Ok(())
    }

    /// Send the media behind `link`, returning whether it came from the cache.
    async fn deliver(
        &self,
        bot: &Bot,
        msg: &Message,
        link: &MediaLink,
        cache: &MediaCache,
    ) -> Result<bool> {
        let started_at = Instant::now();
        info!(platform = %link.platform, "Handling media URL");
        match self.try_send_cached(bot, msg, link, cache).await {
            Ok(true) => {
                info!(
                    platform = %link.platform,
                    cache_hit = true,
                    elapsed = ?started_at.elapsed(),
                    "Handled media URL"
                );
                return Ok(true);
            }
            Ok(false) => {}
            Err(err) => {
                warn!(
                    platform = %link.platform,
                    %err,
                    "Cached media send failed; invalidating entry and falling back to download"
                );
                if let Err(err) = cache.invalidate(&link.cache_key).await {
                    warn!(platform = %link.platform, %err, "Failed to invalidate the cache entry");
                }
            }
        }

        let download = Arc::new(
            self.downloader
                .download(link.platform, &link.original_url)
                .await?,
        );
        let media_items = download.supported_media().await?;
        let source_text = download.source_text.as_deref();
        let media_count = media_items.len();
        let include_source_text =
            should_include_source_text(link.platform, &media_items, source_text);

        debug!(
            platform = %link.platform,
            media_count,
            has_source_text = source_text.is_some(),
            elapsed = ?started_at.elapsed(),
            "Prepared downloaded media"
        );

        #[cfg(feature = "llm-captions")]
        let (base_caption, late_caption) = self
            .initial_caption(
                bot,
                msg,
                link.platform,
                JobMedia::Downloaded {
                    download: Arc::clone(&download),
                    items: media_items.clone(),
                },
            )
            .await
            .or_static(|| self.comments.build_caption());
        #[cfg(not(feature = "llm-captions"))]
        let base_caption = self.comments.build_caption();

        let caption = if include_source_text {
            compose_caption(&base_caption, source_text)
        } else {
            base_caption
        };
        let inputs = media_items
            .iter()
            .map(|(path, kind)| (InputFile::file(path), *kind))
            .collect::<Vec<_>>();
        let sent = send_media(bot, msg.chat.id, inputs, &caption).await?;
        if let Err(err) = cache.put(&link.cache_key, &sent.cached).await {
            warn!(platform = %link.platform, %err, "Failed to persist media file_ids");
        }

        #[cfg(feature = "llm-captions")]
        if let Some(late_caption) = late_caption {
            late_caption.apply_later(
                bot.clone(),
                msg.chat.id,
                sent.captioned,
                source_text
                    .filter(|_| include_source_text)
                    .map(str::to_owned),
            );
        }

        info!(
            platform = %link.platform,
            media_count,
            cache_hit = false,
            elapsed = ?started_at.elapsed(),
            "Handled media URL"
        );
        Ok(false)
    }

    /// Send media from cached Telegram `file_id`s for a link's cache key.
    ///
    /// Returns `Ok(false)` when the key has no cache entry or the lookup
    /// fails; send failures propagate so the caller can invalidate the entry
    /// and fall back to a fresh download.
    async fn try_send_cached(
        &self,
        bot: &Bot,
        msg: &Message,
        link: &MediaLink,
        cache: &MediaCache,
    ) -> Result<bool> {
        let cached = match cache.get(&link.cache_key).await {
            Ok(cached) => cached,
            Err(err) => {
                warn!(platform = %link.platform, %err, "Media cache lookup failed");
                return Ok(false);
            }
        };
        let Some(cached) = cached else {
            return Ok(false);
        };

        let media_count = cached.len();
        #[cfg(feature = "llm-captions")]
        let (caption, late_caption) = self
            .initial_caption(bot, msg, link.platform, JobMedia::Cached(cached.clone()))
            .await
            .or_static(|| self.comments.build_caption());
        #[cfg(not(feature = "llm-captions"))]
        let caption = self.comments.build_caption();
        let inputs = cached
            .into_iter()
            .map(|item| (InputFile::file_id(FileId(item.file_id)), item.kind))
            .collect::<Vec<_>>();
        #[cfg_attr(
            not(feature = "llm-captions"),
            expect(unused_variables, reason = "only LLM captions edit sent captions")
        )]
        let sent = send_media(bot, msg.chat.id, inputs, &caption).await?;
        debug!(platform = %link.platform, media_count, "Sent cached media");

        #[cfg(feature = "llm-captions")]
        if let Some(late_caption) = late_caption {
            late_caption.apply_later(bot.clone(), msg.chat.id, sent.captioned, None);
        }
        Ok(true)
    }

    /// Start an LLM caption for `media` and wait the configured time for it.
    #[cfg(feature = "llm-captions")]
    async fn initial_caption(
        &self,
        bot: &Bot,
        msg: &Message,
        platform: Platform,
        media: JobMedia,
    ) -> InitialCaption {
        let Some(captioner) = &self.captioner else {
            return InitialCaption::Unavailable;
        };
        CaptionJob {
            captioner: captioner.clone(),
            bot: bot.clone(),
            platform,
            sender_name: msg.from.as_ref().map(|user| user.first_name.clone()),
            message_text: msg.text().or_else(|| msg.caption()).map(str::to_owned),
            media,
        }
        .start()
        .wait(captioner.initial_wait())
        .await
    }
}

impl Handler {
    fn new(platform: Platform, regex_pattern: &str) -> StdResult<Self, RegexError> {
        let regex = Regex::new(regex_pattern)?;
        Ok(Self { platform, regex })
    }

    const fn platform(&self) -> Platform {
        self.platform
    }

    fn try_extract_all(&self, text: &str, source_offset: usize) -> Vec<MediaLink> {
        self.regex
            .captures_iter(text)
            .filter_map(|captures| {
                let matched = captures.name("url").or_else(|| captures.get(0))?;
                let original_url = matched.as_str().to_owned();
                let cache_key = normalize_cache_key(self.platform, &original_url);
                Some(MediaLink {
                    platform: self.platform,
                    original_url,
                    cache_key,
                    source_position: source_offset.saturating_add(matched.start()),
                })
            })
            .collect()
    }
}

fn create_handlers(platforms: &PlatformConfig) -> StdResult<Arc<[Handler]>, RegexError> {
    let handlers = [
        Handler::new(
            Platform::Instagram,
            r"(?P<url>https?://(?:www\.)?(?:instagram\.com|instagr\.am)/(?:reel|tv|(?:[A-Za-z0-9._]+/)?p)/[A-Za-z0-9_-]+/?(?:\?[^\s#]*)?(?:#[^\s]*)?)(?:[^\w/?#-]|$)",
        ),
        Handler::new(
            Platform::Youtube,
            r"https?://(?:www\.)?youtube\.com\/shorts\/[A-Za-z0-9_-]+(?:\?[^\s]*)?",
        ),
        Handler::new(
            Platform::Twitter,
            r"https?://(?:www\.)?(?:twitter\.com|x\.com)/([A-Za-z0-9_]+(?:/[A-Za-z0-9_]+)?)/status/(\d{1,20})",
        ),
        Handler::new(
            Platform::Tiktok,
            r"https?://(?:(?:www\.)?tiktok\.com/@[A-Za-z0-9._-]+/video/\d+(?:\?[^\s]*)?|(?:vm|vt|tt|tik)\.tiktok\.com/[A-Za-z0-9_-]+[/?#]?)",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CobaltConfig;
    use claims::{assert_ok, assert_some};

    fn all_handlers() -> MediaHandlers {
        let downloader = assert_ok!(Downloader::new(CobaltConfig::default()));
        assert_ok!(MediaHandlers::new(
            &PlatformConfig::default(),
            downloader,
            Arc::new(Comments::default()),
        ))
    }

    fn instagram_handler(handlers: &MediaHandlers) -> &Handler {
        assert_some!(
            handlers
                .handlers
                .iter()
                .find(|handler| handler.platform() == Platform::Instagram)
        )
    }

    fn original_urls(handler: &Handler, text: &str) -> Vec<String> {
        handler
            .try_extract_all(text, 0)
            .into_iter()
            .map(|link| link.original_url)
            .collect()
    }

    #[test]
    fn extracts_tiktok_profile_video_url_with_query() {
        let handlers = all_handlers();
        let handler = assert_some!(
            handlers
                .handlers
                .iter()
                .find(|handler| handler.platform() == Platform::Tiktok)
        );
        let url = "https://www.tiktok.com/@apple/video/7673917526358641950?is_from_webapp=1&sender_device=pc";

        let links = handler.try_extract_all(url, 0);
        assert_eq!(
            links.first().map(|link| link.original_url.as_str()),
            Some(url)
        );
    }
    #[test]
    fn extracts_instagram_post_urls() {
        let handlers = all_handlers();
        let handler = instagram_handler(&handlers);

        for url in [
            "https://instagram.com/p/ABC123",
            "https://instagr.am/p/ABC123",
            "https://www.instagram.com/f1/p/DcyZobPjRyI/",
            "https://www.instagram.com/p/Dfevru5oYgb/?xtok=dHpmODA1ampua2lp",
        ] {
            assert_eq!(original_urls(handler, url), [url]);
        }
    }
    #[test]
    fn existing_instagram_reel_and_tv_urls_still_match_with_query_and_fragment() {
        let handlers = all_handlers();
        let handler = instagram_handler(&handlers);

        for url in [
            "https://www.instagram.com/reel/ABC123/?utm_source=share#carousel",
            "https://instagr.am/tv/ABC123?utm_source=share#tv",
        ] {
            assert_eq!(original_urls(handler, url), [url]);
        }
    }
    #[test]
    fn rejects_non_instagram_paths_and_malformed_urls() {
        let handlers = all_handlers();
        let handler = instagram_handler(&handlers);

        for url in [
            "https://www.instagram.com/photographer",
            "https://www.instagram.com/stories/photographer/123",
            "https://www.instagram.com/accounts/login",
            "https://www.instagram.com/reel/",
            "https://www.instagram.com/tv/ABC123/extra",
        ] {
            assert_eq!(original_urls(handler, url), Vec::<String>::new());
        }
    }
}
