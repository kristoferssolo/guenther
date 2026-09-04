mod delivery;

use self::delivery::{compose_caption, send_media, should_include_source_text};
use crate::media_link::{MediaLink, normalize_cache_key};
use guenther::{
    cache::MediaCache,
    comments::Comments,
    config::{Platform, PlatformConfig},
    download::{collect_supported_media, platform::Downloader},
    error::Result,
};
use regex::{Error as RegexError, Regex};
use std::{result::Result as StdResult, sync::Arc, time::Instant};
use teloxide::{
    Bot,
    types::{ChatId, FileId, InputFile},
};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct Handler {
    platform: Platform,
    regex: Regex,
}

/// Matches supported URLs and dispatches them to the configured downloader.
#[derive(Debug, Clone)]
pub struct MediaHandlers {
    handlers: Arc<[Handler]>,
    downloader: Downloader,
    comments: Arc<Comments>,
}

impl MediaHandlers {
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
        })
    }

    pub fn enabled_platforms(&self) -> impl Iterator<Item = Platform> + '_ {
        self.handlers.iter().map(Handler::platform)
    }

    pub fn extract(&self, text: Option<&str>, caption: Option<&str>) -> Vec<MediaLink> {
        crate::media_link::extract_media_links(text, caption, &self.handlers)
    }

    pub async fn handle(
        &self,
        bot: &Bot,
        chat_id: ChatId,
        link: &MediaLink,
        cache: &MediaCache,
    ) -> Result<()> {
        let Some(handler) = self
            .handlers
            .iter()
            .find(|handler| handler.platform() == link.platform)
        else {
            return Ok(());
        };
        handler
            .handle(bot, chat_id, link, cache, &self.downloader, &self.comments)
            .await
    }
}

impl Handler {
    pub fn new(platform: Platform, regex_pattern: &str) -> StdResult<Self, RegexError> {
        let regex = Regex::new(regex_pattern)?;
        Ok(Self { platform, regex })
    }

    pub const fn platform(&self) -> Platform {
        self.platform
    }

    #[cfg(test)]
    pub fn try_extract<'a>(&self, text: &'a str) -> Option<&'a str> {
        self.regex
            .captures(text)
            .and_then(|captures| captures.name("url").or_else(|| captures.get(0)))
            .map(|matched| matched.as_str())
    }

    pub fn try_extract_all(&self, text: &str, source_offset: usize) -> Vec<MediaLink> {
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

    pub async fn handle(
        &self,
        bot: &Bot,
        chat_id: ChatId,
        link: &MediaLink,
        cache: &MediaCache,
        downloader: &Downloader,
        comments: &Comments,
    ) -> Result<()> {
        let started_at = Instant::now();
        info!(platform = %self.platform, "Handling media URL");
        match self
            .try_send_cached(bot, chat_id, link.cache_key.as_str(), cache, comments)
            .await
        {
            Ok(true) => {
                info!(
                    platform = %self.platform,
                    cache_hit = true,
                    elapsed = ?started_at.elapsed(),
                    "Handled media URL"
                );
                return Ok(());
            }
            Ok(false) => {}
            Err(err) => {
                warn!(
                    platform = %self.platform,
                    %err,
                    "Cached media send failed; invalidating entry and falling back to download"
                );
                if let Err(err) = cache.invalidate(&link.cache_key).await {
                    warn!(platform = %self.platform, %err, "Failed to invalidate the cache entry");
                }
            }
        }

        let mut dr = downloader
            .download(self.platform, &link.original_url)
            .await?;
        let source_text = dr.source_text.take();
        let (_tempdir, media_items) = collect_supported_media(dr).await?;
        let media_count = media_items.len();
        let base_caption = comments.build_caption();
        let include_source_text =
            should_include_source_text(self.platform(), &media_items, source_text.as_deref());

        debug!(
            platform = %self.platform,
            media_count,
            has_source_text = source_text.is_some(),
            elapsed = ?started_at.elapsed(),
            "Prepared downloaded media"
        );

        let caption = if include_source_text {
            compose_caption(&base_caption, source_text.as_deref())
        } else {
            base_caption
        };
        let inputs = media_items
            .into_iter()
            .map(|(path, kind)| (InputFile::file(path), kind))
            .collect::<Vec<_>>();
        let cached = send_media(bot, chat_id, inputs, &caption).await?;
        if let Err(err) = cache.put(&link.cache_key, &cached).await {
            warn!(platform = %self.platform, %err, "Failed to persist media file_ids");
        }

        info!(
            platform = %self.platform,
            media_count,
            cache_hit = false,
            elapsed = ?started_at.elapsed(),
            "Handled media URL"
        );
        Ok(())
    }

    /// Send media from cached Telegram `file_id`s for a cache key.
    ///
    /// Returns `Ok(false)` when the key has no cache entry or the lookup
    /// fails; send failures propagate so the caller can invalidate the entry
    /// and fall back to a fresh download.
    async fn try_send_cached(
        &self,
        bot: &Bot,
        chat_id: ChatId,
        cache_key: &str,
        cache: &MediaCache,
        comments: &Comments,
    ) -> Result<bool> {
        let cached = match cache.get(cache_key).await {
            Ok(cached) => cached,
            Err(err) => {
                warn!(platform = %self.platform, %err, "Media cache lookup failed");
                return Ok(false);
            }
        };
        let Some(cached) = cached else {
            return Ok(false);
        };

        let media_count = cached.len();
        let inputs = cached
            .into_iter()
            .map(|item| (InputFile::file_id(FileId(item.file_id)), item.kind))
            .collect::<Vec<_>>();
        let caption = comments.build_caption();
        send_media(bot, chat_id, inputs, &caption).await?;
        debug!(platform = %self.platform, media_count, "Sent cached media");
        Ok(true)
    }
}

fn create_handlers(platforms: &PlatformConfig) -> StdResult<Arc<[Handler]>, RegexError> {
    let handlers = [
        Handler::new(
            Platform::Instagram,
            r"(?P<url>https?://(?:www\.)?(?:instagram\.com|instagr\.am)/(?:reel|tv)/[A-Za-z0-9_-]+/?(?:\?[^\s#]*)?(?:#[^\s]*)?)(?:[^\w/?#-]|$)",
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
    use claims::{assert_none, assert_ok, assert_some};
    use guenther::config::CobaltConfig;

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
    fn ignores_instagram_post_urls() {
        let handlers = all_handlers();
        let handler = instagram_handler(&handlers);
        let urls = [
            "https://instagram.com/p/ABC123",
            "https://instagr.am/p/ABC123",
            "https://www.instagram.com/f1/p/DcyZobPjRyI/",
        ];

        for url in urls {
            assert_none!(handler.try_extract(url));
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
            assert_eq!(handler.try_extract(url), Some(url));
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
            assert_none!(handler.try_extract(url));
        }
    }
}
