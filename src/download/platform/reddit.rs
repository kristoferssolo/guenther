use crate::{
    download::{DownloadResult, platform::cobalt::download_with_cobalt_converting_gifs},
    error::Result,
};
use url::Url;

/// A recognized Reddit post URL reduced to its stable post identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedditUrl {
    post_id: String,
    share_link: bool,
}

impl RedditUrl {
    /// Parse a Reddit post URL without making any network requests.
    pub fn parse(source: &str) -> Option<Self> {
        let url = Url::parse(source).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }

        let host = url.host_str()?;
        let mut segments = url.path_segments()?;
        let (post_id, share_link) = if is_reddit_host(host) {
            let (Some("r"), Some(subreddit), Some(link_type), Some(post_id)) = (
                segments.next(),
                segments.next(),
                segments.next(),
                segments.next(),
            ) else {
                return None;
            };
            if !valid_segment(subreddit) {
                return None;
            }
            let share_link = match link_type {
                "comments" => false,
                "s" => true,
                _ => return None,
            };
            (post_id, share_link)
        } else if host.eq_ignore_ascii_case("redd.it") {
            let path = url.path().strip_suffix('/').unwrap_or_else(|| url.path());
            let post_id = path.strip_prefix('/')?;
            if post_id.is_empty() || post_id.contains('/') {
                return None;
            }
            (post_id, false)
        } else {
            return None;
        };

        valid_post_id(post_id).then(|| Self {
            post_id: post_id.to_owned(),
            share_link,
        })
    }

    /// Return the Reddit post identifier.
    pub fn post_id(&self) -> &str {
        &self.post_id
    }

    /// Return the stable cache key for this post.
    pub fn cache_key(&self) -> String {
        if self.share_link {
            format!("reddit:share:{}", self.post_id)
        } else {
            format!("reddit:{}", self.post_id)
        }
    }
}

/// Normalize equivalent Reddit post URLs into a stable cache key.
pub fn normalize_reddit_url(source: &str) -> Option<String> {
    RedditUrl::parse(source).map(|url| url.cache_key())
}

/// Download Reddit-hosted media through the configured private Cobalt instance.
/// Animated GIFs are requested as video so Telegram can send them as video.
///
/// # Errors
///
/// Propagates Cobalt API and media download errors.
pub async fn download_reddit(url: String) -> Result<DownloadResult> {
    download_with_cobalt_converting_gifs(&url).await
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn is_reddit_host(host: &str) -> bool {
    [
        "reddit.com",
        "www.reddit.com",
        "old.reddit.com",
        "new.reddit.com",
        "m.reddit.com",
    ]
    .into_iter()
    .any(|candidate| host.eq_ignore_ascii_case(candidate))
}

fn valid_post_id(post_id: &str) -> bool {
    !post_id.is_empty() && post_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some_eq};

    #[test]
    fn parses_normal_post_url() {
        let url = RedditUrl::parse(
            "https://www.reddit.com/r/rust/comments/abc123/a_title/?utm_source=chat",
        );
        let url = url.as_ref();

        assert_some_eq!(url.map(RedditUrl::post_id), "abc123");
        assert_some_eq!(url.map(RedditUrl::cache_key), "reddit:abc123");
    }

    #[test]
    fn normalizes_equivalent_post_urls_by_id() {
        let canonical = "reddit:abc123";
        for source in [
            "https://reddit.com/r/rust/comments/abc123/title",
            "https://old.reddit.com/r/other/comments/abc123/different-title?sort=top",
            "https://m.reddit.com/r/rust/comments/abc123/",
            "https://redd.it/abc123/",
        ] {
            assert_eq!(normalize_reddit_url(source).as_deref(), Some(canonical));
        }
    }

    #[test]
    fn recognizes_share_links_without_resolving_them() {
        let source = "https://www.reddit.com/r/europe/s/2VOdpFNS8p";

        assert_eq!(
            normalize_reddit_url(source).as_deref(),
            Some("reddit:share:2VOdpFNS8p")
        );
    }

    #[test]
    fn preserves_short_link_identifier_without_resolution() {
        let source = "https://redd.it/abc123";

        assert_eq!(
            normalize_reddit_url(source).as_deref(),
            Some("reddit:abc123")
        );
        assert_some_eq!(
            RedditUrl::parse(source).map(|url| url.post_id().to_owned()),
            "abc123"
        );
    }

    #[test]
    fn rejects_malformed_or_unrecognized_urls() {
        for source in [
            "https://reddit.com/r/rust/comments/",
            "https://reddit.com/r/rust/post/abc123",
            "https://reddit.com/r/rust/comments/abc-123/title",
            "https://reddit.com/r/rust/s/abc-123",
            "https://reddit.com.evil/r/rust/comments/abc123/title",
            "https://example.com/r/rust/comments/abc123/title",
            "https://reddit.com:8080/r/rust/comments/abc123/title",
            "https://redd.it/abc123/extra",
        ] {
            assert_none!(RedditUrl::parse(source));
        }
    }
}
