use crate::{
    config::{Platform, PlatformConfig},
    download::platform::twitter_status_id,
};
use regex::{Error as RegexError, Regex};
use std::{collections::HashSet, sync::Arc};
use url::Url;

const URL_PATTERNS: [(Platform, &str); 4] = [
    (
        Platform::Instagram,
        r"(?P<url>https?://(?:www\.)?(?:instagram\.com|instagr\.am)/(?:reel|tv|(?:[A-Za-z0-9._]+/)?p)/[A-Za-z0-9_-]+/?(?:\?[^\s#]*)?(?:#[^\s]*)?)(?:[^\w/?#-]|$)",
    ),
    (
        Platform::Youtube,
        r"https?://(?:www\.)?youtube\.com\/shorts\/[A-Za-z0-9_-]+(?:\?[^\s]*)?",
    ),
    (
        Platform::Twitter,
        r"https?://(?:www\.)?(?:twitter\.com|x\.com)/([A-Za-z0-9_]+(?:/[A-Za-z0-9_]+)?)/status/(\d{1,20})",
    ),
    (
        Platform::Tiktok,
        r"https?://(?:(?:www\.)?tiktok\.com/@[A-Za-z0-9._-]+/video/\d+(?:\?[^\s]*)?|(?:vm|vt|tt|tik)\.tiktok\.com/[A-Za-z0-9_-]+[/?#]?)",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaLink {
    pub platform: Platform,
    pub original_url: String,
    pub cache_key: String,
    pub source_position: usize,
}

/// Finds supported media links for the enabled platforms.
#[derive(Debug, Clone)]
pub struct LinkExtractor {
    patterns: Arc<[LinkPattern]>,
}

impl LinkExtractor {
    /// Compile the URL patterns of the enabled platforms.
    ///
    /// # Errors
    ///
    /// Returns an error if a platform URL pattern fails to compile.
    pub fn new(platforms: &PlatformConfig) -> Result<Self, RegexError> {
        let patterns = URL_PATTERNS
            .into_iter()
            .map(|(platform, pattern)| LinkPattern::new(platform, pattern))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            patterns: patterns
                .into_iter()
                .filter(|pattern| platforms.is_enabled(pattern.platform))
                .collect(),
        })
    }

    pub fn platforms(&self) -> impl Iterator<Item = Platform> + '_ {
        self.patterns.iter().map(|pattern| pattern.platform)
    }

    /// Supported links in `text`, then `caption`, in textual order and
    /// deduplicated by cache key.
    pub fn extract(&self, text: Option<&str>, caption: Option<&str>) -> Vec<MediaLink> {
        let mut links = Vec::new();
        let mut source_offset = 0;

        for source in [text, caption].into_iter().flatten() {
            links.extend(
                self.patterns
                    .iter()
                    .flat_map(|pattern| pattern.find_all(source, source_offset)),
            );
            source_offset = source_offset.saturating_add(source.len()).saturating_add(1);
        }

        links.sort_unstable_by_key(|link| link.source_position);
        let mut seen_cache_keys = HashSet::new();
        links.retain(|link| seen_cache_keys.insert(link.cache_key.clone()));
        links
    }
}

#[derive(Debug)]
struct LinkPattern {
    platform: Platform,
    regex: Regex,
}

impl LinkPattern {
    fn new(platform: Platform, pattern: &str) -> Result<Self, RegexError> {
        Ok(Self {
            platform,
            regex: Regex::new(pattern)?,
        })
    }

    /// Links matched in `text`, positioned `source_offset` bytes into the
    /// message.
    fn find_all<'a>(
        &'a self,
        text: &'a str,
        source_offset: usize,
    ) -> impl Iterator<Item = MediaLink> + 'a {
        self.regex.captures_iter(text).filter_map(move |captures| {
            let matched = captures.name("url").or_else(|| captures.get(0))?;
            let original_url = matched.as_str().to_owned();
            Some(MediaLink {
                platform: self.platform,
                cache_key: normalize_cache_key(self.platform, &original_url),
                original_url,
                source_position: source_offset.saturating_add(matched.start()),
            })
        })
    }
}

fn normalize_cache_key(platform: Platform, original_url: &str) -> String {
    let Ok(url) = Url::parse(original_url) else {
        return format!(
            "{platform}:url:{}",
            original_url
                .split(['?', '#'])
                .next()
                .unwrap_or(original_url)
                .trim_end_matches('/')
        );
    };

    let stable_key = match platform {
        Platform::Instagram => instagram_media_key(&url),
        Platform::Tiktok => tiktok_video_key(&url),
        Platform::Twitter => twitter_status_key(&url),
        Platform::Youtube => youtube_video_key(&url),
    };
    stable_key.unwrap_or_else(|| format!("{platform}:url:{}", canonical_url(&url)))
}

/// Reels, TV posts, and `/p/` posts share one shortcode namespace, so all of
/// them map to the same key.
fn instagram_media_key(url: &Url) -> Option<String> {
    let mut segments = path_segments(url);
    let ((Some("reel" | "tv" | "p"), Some(identifier), _) | (Some(_), Some("p"), Some(identifier))) =
        (segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    Some(format!("instagram:post:{identifier}"))
}

/// Whether `original_url` is an Instagram reel or TV post, which should
/// arrive as video.
pub fn is_instagram_video_url(original_url: &str) -> bool {
    Url::parse(original_url)
        .is_ok_and(|url| matches!(path_segments(&url).next(), Some("reel" | "tv")))
}

fn tiktok_video_key(url: &Url) -> Option<String> {
    let mut segments = path_segments(url);
    if let (Some(user), Some("video"), Some(identifier)) =
        (segments.next(), segments.next(), segments.next())
        && user.starts_with('@')
    {
        return Some(format!("tiktok:video:{identifier}"));
    }

    let host = url.host_str()?.to_ascii_lowercase();
    if matches!(
        host.as_str(),
        "vm.tiktok.com" | "vt.tiktok.com" | "tt.tiktok.com" | "tik.tiktok.com"
    ) {
        return path_segments(url)
            .next()
            .map(|identifier| format!("tiktok:short:{identifier}"));
    }

    None
}

fn twitter_status_key(url: &Url) -> Option<String> {
    twitter_status_id(url).map(|identifier| format!("twitter:status:{identifier}"))
}

fn youtube_video_key(url: &Url) -> Option<String> {
    let mut segments = path_segments(url);
    match (segments.next(), segments.next()) {
        (Some("shorts"), Some(identifier)) => Some(format!("youtube:shorts:{identifier}")),
        _ => None,
    }
}

fn path_segments(url: &Url) -> impl Iterator<Item = &str> {
    url.path_segments().into_iter().flatten()
}

fn canonical_url(url: &Url) -> String {
    let mut url = url.clone();
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_owned();
    if path.is_empty() {
        url.set_path("/");
    } else {
        url.set_path(&path);
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use rstest::rstest;

    fn extractor() -> LinkExtractor {
        assert_ok!(LinkExtractor::new(&PlatformConfig::default()))
    }

    fn original_urls(text: &str) -> Vec<String> {
        extractor()
            .extract(Some(text), None)
            .into_iter()
            .map(|link| link.original_url)
            .collect()
    }

    #[test]
    fn extracts_tiktok_profile_video_url_with_query() {
        let url = "https://www.tiktok.com/@apple/video/7673917526358641950?is_from_webapp=1&sender_device=pc";

        assert_eq!(original_urls(url), [url]);
    }

    #[test]
    fn extracts_instagram_post_urls() {
        for url in [
            "https://instagram.com/p/ABC123",
            "https://instagr.am/p/ABC123",
            "https://www.instagram.com/f1/p/DcyZobPjRyI/",
            "https://www.instagram.com/p/Dfevru5oYgb/?xtok=dHpmODA1ampua2lp",
        ] {
            assert_eq!(original_urls(url), [url]);
        }
    }

    #[test]
    fn existing_instagram_reel_and_tv_urls_still_match_with_query_and_fragment() {
        for url in [
            "https://www.instagram.com/reel/ABC123/?utm_source=share#carousel",
            "https://instagr.am/tv/ABC123?utm_source=share#tv",
        ] {
            assert_eq!(original_urls(url), [url]);
        }
    }

    #[test]
    fn rejects_non_instagram_paths_and_malformed_urls() {
        for url in [
            "https://www.instagram.com/photographer",
            "https://www.instagram.com/stories/photographer/123",
            "https://www.instagram.com/accounts/login",
            "https://www.instagram.com/reel/",
            "https://www.instagram.com/tv/ABC123/extra",
        ] {
            assert_eq!(original_urls(url), Vec::<String>::new());
        }
    }

    #[test]
    fn extracts_several_supported_urls_from_one_message() {
        let extractor = extractor();
        let text = concat!(
            "https://instagram.com/reel/instagram-123 ",
            "https://www.youtube.com/shorts/youtube_456 ",
            "https://x.com/driver/status/789 ",
            "https://www.tiktok.com/@driver/video/987654321"
        );

        let links = extractor.extract(Some(text), None);

        assert_eq!(links.len(), 4);
        assert_eq!(
            links.iter().map(|link| link.platform).collect::<Vec<_>>(),
            vec![
                Platform::Instagram,
                Platform::Youtube,
                Platform::Twitter,
                Platform::Tiktok
            ]
        );
    }

    #[test]
    fn preserves_textual_order_across_platforms() {
        let extractor = extractor();
        let text = concat!(
            "https://x.com/driver/status/123 before ",
            "https://instagram.com/reel/abc123 after"
        );

        let links = extractor.extract(Some(text), None);

        assert_eq!(
            links.iter().map(|link| link.platform).collect::<Vec<_>>(),
            vec![Platform::Twitter, Platform::Instagram]
        );
    }

    #[test]
    fn deduplicates_tracking_variants_by_cache_key() {
        let extractor = extractor();
        let text = concat!(
            "https://www.youtube.com/shorts/video-123?si=first ",
            "https://youtube.com/shorts/video-123?feature=share#comments"
        );

        let links = extractor.extract(Some(text), None);

        assert_eq!(links.len(), 1);
        assert_eq!(
            links.first().map(|link| link.original_url.as_str()),
            Some("https://www.youtube.com/shorts/video-123?si=first")
        );
    }

    #[test]
    fn extracts_links_from_captions() {
        let extractor = extractor();

        let links = extractor.extract(None, Some("A caption with https://x.com/driver/status/123"));

        assert_eq!(links.len(), 1);
        assert_eq!(
            links.first().map(|link| link.platform),
            Some(Platform::Twitter)
        );
    }

    #[test]
    fn cache_key_normalizes_equivalent_url_variants() {
        assert_eq!(
            normalize_cache_key(
                Platform::Twitter,
                "https://twitter.com/driver/status/123?utm_source=telegram#replies"
            ),
            normalize_cache_key(Platform::Twitter, "https://x.com/another-name/status/123")
        );
        assert_eq!(
            normalize_cache_key(
                Platform::Youtube,
                "https://www.youtube.com/shorts/video-123?si=tracking#comments"
            ),
            "youtube:shorts:video-123"
        );
        assert_eq!(
            normalize_cache_key(
                Platform::Instagram,
                "https://www.instagram.com/reel/reel-123?utm_source=share#comments"
            ),
            "instagram:post:reel-123"
        );
        for url in [
            "https://www.instagram.com/p/post-123/?xtok=tracking",
            "https://www.instagram.com/f1/p/post-123",
            "https://www.instagram.com/reel/post-123",
        ] {
            assert_eq!(
                normalize_cache_key(Platform::Instagram, url),
                "instagram:post:post-123"
            );
        }
    }

    #[test]
    fn ignores_messages_without_supported_urls() {
        let extractor = extractor();

        let links = extractor.extract(
            Some("No media here"),
            Some("https://example.com/not-supported"),
        );

        assert_eq!(links, Vec::<MediaLink>::new());
    }

    #[rstest]
    #[case::reel("https://www.instagram.com/reel/ABC123/?utm_source=share", true)]
    #[case::tv("https://instagr.am/tv/ABC123", true)]
    #[case::post("https://www.instagram.com/p/ABC123/", false)]
    #[case::profile_post("https://www.instagram.com/f1/p/ABC123", false)]
    #[case::not_a_url("reel/ABC123", false)]
    fn detects_instagram_video_urls(#[case] url: &str, #[case] expected: bool) {
        assert_eq!(is_instagram_video_url(url), expected);
    }
}
