use crate::{
    cache::{CacheError, MediaCache},
    config::Platform,
    handler::LinkExtractor,
    telemetry::ErrorChain,
};
use std::fmt::{self, Display, Formatter};
use strum::VariantArray;
use thiserror::Error;
use tracing::{error, info};

#[derive(Debug, Error)]
pub enum PurgeError {
    #[error("Usage: /purge <url|platform|all>")]
    MissingTarget,

    #[error(
        "`{0}` is not a supported media link or platform. Use one link, a platform ({platforms}), or `all`.",
        platforms = platform_names()
    )]
    UnsupportedTarget(String),

    #[error("Failed to purge the media cache: {0}")]
    Cache(#[from] CacheError),
}

/// Which `media_cache` rows a `/purge` command deletes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeTarget {
    All,
    Platform(Platform),
    CacheKey(String),
}

impl PurgeTarget {
    /// Parse a `/purge` argument: `all`, a platform name, or one media link.
    ///
    /// Links are normalized to their cache key with the enabled `links`, so
    /// links of disabled platforms are unsupported; their platform name still
    /// works.
    ///
    /// # Errors
    ///
    /// Returns an error when the argument is empty, or is not `all`, a
    /// platform name, or exactly one supported link.
    pub fn parse(input: &str, links: &LinkExtractor) -> Result<Self, PurgeError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(PurgeError::MissingTarget);
        }
        if input.eq_ignore_ascii_case("all") {
            return Ok(Self::All);
        }
        if let Ok(platform) = input.parse() {
            return Ok(Self::Platform(platform));
        }
        // `extract` skips unsupported links and merges duplicates, so a
        // multi-word argument could otherwise purge only one of its links.
        if input.contains(char::is_whitespace) {
            return Err(PurgeError::UnsupportedTarget(input.to_owned()));
        }
        match links.extract(Some(input), None).as_slice() {
            [link] => Ok(Self::CacheKey(link.cache_key.clone())),
            _ => Err(PurgeError::UnsupportedTarget(input.to_owned())),
        }
    }

    /// Delete the matching rows and return how many were deleted.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails.
    pub async fn purge(&self, cache: &MediaCache) -> Result<u64, CacheError> {
        match self {
            Self::All => cache.clear().await,
            Self::Platform(platform) => cache.invalidate_platform(*platform).await,
            Self::CacheKey(cache_key) => cache.invalidate(cache_key).await,
        }
    }
}

impl Display for PurgeTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all platforms"),
            Self::Platform(platform) => write!(f, "{platform}"),
            Self::CacheKey(cache_key) => write!(f, "`{cache_key}`"),
        }
    }
}

/// Run `/purge <input>` against `cache` and return the reply text.
///
/// # Errors
///
/// Returns an error when the argument is invalid or the database query fails.
pub async fn purge(
    input: &str,
    links: &LinkExtractor,
    cache: &MediaCache,
) -> Result<String, PurgeError> {
    let target = PurgeTarget::parse(input, links)?;
    let deleted = target.purge(cache).await.inspect_err(
        |err| error!(error = %ErrorChain(err), %target, "Failed to purge the media cache"),
    )?;
    info!(%target, deleted, "Purged the media cache");
    let rows = if deleted == 1 { "row" } else { "rows" };
    Ok(format!(
        "Deleted {deleted} cached media {rows} for {target}."
    ))
}

fn platform_names() -> String {
    Platform::VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cache::CachedMedia, config::PlatformConfig, db, utils::MediaKind};
    use claims::{assert_none, assert_ok, assert_ok_eq, assert_some};
    use rstest::rstest;
    use std::assert_matches;

    fn links() -> LinkExtractor {
        assert_ok!(LinkExtractor::new(&PlatformConfig::default()))
    }

    async fn cache() -> MediaCache {
        MediaCache::new(assert_ok!(db::connect("sqlite::memory:").await))
    }

    fn video(file_id: &str) -> CachedMedia {
        CachedMedia {
            kind: MediaKind::Video,
            file_id: file_id.to_owned(),
        }
    }

    #[rstest]
    #[case::all("all", PurgeTarget::All)]
    #[case::all_uppercase(" ALL ", PurgeTarget::All)]
    #[case::platform(" instagram ", PurgeTarget::Platform(Platform::Instagram))]
    #[case::platform_alias("x", PurgeTarget::Platform(Platform::Twitter))]
    #[case::instagram_link(
        "https://www.instagram.com/reel/abc/?utm_source=share",
        PurgeTarget::CacheKey("instagram:post:abc".to_owned())
    )]
    #[case::twitter_link(
        "https://twitter.com/driver/status/123?s=20",
        PurgeTarget::CacheKey("twitter:status:123".to_owned())
    )]
    fn parses_targets(#[case] input: &str, #[case] expected: PurgeTarget) {
        assert_ok_eq!(PurgeTarget::parse(input, &links()), expected);
    }

    #[rstest]
    #[case::empty("")]
    #[case::blank("   ")]
    fn rejects_missing_targets(#[case] input: &str) {
        assert_matches!(
            PurgeTarget::parse(input, &links()),
            Err(PurgeError::MissingTarget)
        );
    }

    #[rstest]
    #[case::unknown_platform("facebook")]
    #[case::unsupported_link("https://example.com/video/1")]
    #[case::several_links("https://x.com/a/status/1 https://x.com/b/status/2")]
    #[case::supported_and_unsupported_links("https://x.com/a/status/1 https://example.com/video/2")]
    #[case::duplicate_links("https://x.com/a/status/1 https://twitter.com/a/status/1")]
    #[case::link_with_text("https://x.com/a/status/1 please")]
    fn rejects_unsupported_targets(#[case] input: &str) {
        let err = PurgeTarget::parse(input, &links());
        assert_matches!(&err, Err(PurgeError::UnsupportedTarget(target)) if target == input);
    }

    #[test]
    fn unsupported_target_error_lists_platforms() {
        let message = PurgeError::UnsupportedTarget("facebook".to_owned()).to_string();
        assert!(message.contains("instagram, tiktok, twitter, youtube"));
    }

    #[tokio::test]
    async fn link_purge_deletes_the_normalized_key() {
        let cache = cache().await;
        assert_ok!(
            cache
                .put("instagram:post:abc", &[video("a"), video("b")])
                .await
        );
        assert_ok!(cache.put("tiktok:video:1", &[video("c")]).await);

        let reply = purge("https://instagram.com/p/abc/", &links(), &cache).await;

        assert_ok_eq!(
            reply,
            "Deleted 2 cached media rows for `instagram:post:abc`."
        );
        assert_none!(assert_ok!(cache.get("instagram:post:abc").await));
        assert_some!(assert_ok!(cache.get("tiktok:video:1").await));
    }

    #[rstest]
    #[case::instagram(Platform::Instagram, "https://instagram.com/reel/abc")]
    #[case::tiktok(Platform::Tiktok, "https://vm.tiktok.com/ZMabc/")]
    #[case::twitter(Platform::Twitter, "https://x.com/driver/status/123")]
    #[case::youtube(Platform::Youtube, "https://youtube.com/shorts/abc")]
    #[tokio::test]
    async fn platform_purge_matches_normalized_keys(#[case] platform: Platform, #[case] url: &str) {
        let links = links();
        let cache = cache().await;
        let extracted = links.extract(Some(url), None);
        let cache_key = &assert_some!(extracted.first()).cache_key;
        assert_ok!(cache.put(cache_key, &[video("a")]).await);

        let reply = purge(&platform.to_string(), &links, &cache).await;

        assert_ok_eq!(reply, format!("Deleted 1 cached media row for {platform}."));
        assert_none!(assert_ok!(cache.get(cache_key).await));
    }
}
