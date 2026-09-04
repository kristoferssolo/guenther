use crate::handler::Handler;
use guenther::config::Platform;
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaLink {
    pub platform: Platform,
    pub original_url: String,
    pub cache_key: String,
    pub source_position: usize,
}

pub fn extract_media_links(
    text: Option<&str>,
    caption: Option<&str>,
    handlers: &[Handler],
) -> Vec<MediaLink> {
    let mut links = Vec::new();
    let mut source_offset = 0;

    for source in [text, caption].into_iter().flatten() {
        links.extend(
            handlers
                .iter()
                .flat_map(|handler| handler.try_extract_all(source, source_offset)),
        );
        source_offset = source_offset.saturating_add(source.len()).saturating_add(1);
    }

    links.sort_unstable_by_key(|link| link.source_position);
    let mut seen_cache_keys = HashSet::new();
    links.retain(|link| seen_cache_keys.insert(link.cache_key.clone()));
    links
}

pub fn normalize_cache_key(platform: Platform, original_url: &str) -> String {
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

fn instagram_media_key(url: &Url) -> Option<String> {
    let mut segments = path_segments(url);
    while let Some(segment) = segments.next() {
        if matches!(segment, "reel" | "tv")
            && let Some(identifier) = segments.next()
        {
            return Some(format!("instagram:post:{identifier}"));
        }
    }
    None
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
    let mut segments = path_segments(url);
    while let Some(segment) = segments.next() {
        if segment == "status"
            && let Some(identifier) = segments.next()
            && identifier.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Some(format!("twitter:status:{identifier}"));
        }
    }
    None
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
    use crate::handler::create_handlers;
    use claims::assert_ok;
    use guenther::config::PlatformConfig;

    #[test]
    fn extracts_several_supported_urls_from_one_message() {
        let handlers = assert_ok!(create_handlers(&PlatformConfig::default()));
        let text = concat!(
            "https://instagram.com/reel/instagram-123 ",
            "https://www.youtube.com/shorts/youtube_456 ",
            "https://x.com/driver/status/789 ",
            "https://www.tiktok.com/@driver/video/987654321"
        );

        let links = extract_media_links(Some(text), None, &handlers);

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
        let handlers = assert_ok!(create_handlers(&PlatformConfig::default()));
        let text = concat!(
            "https://x.com/driver/status/123 before ",
            "https://instagram.com/reel/abc123 after"
        );

        let links = extract_media_links(Some(text), None, &handlers);

        assert_eq!(
            links.iter().map(|link| link.platform).collect::<Vec<_>>(),
            vec![Platform::Twitter, Platform::Instagram]
        );
    }

    #[test]
    fn deduplicates_tracking_variants_by_cache_key() {
        let handlers = assert_ok!(create_handlers(&PlatformConfig::default()));
        let text = concat!(
            "https://www.youtube.com/shorts/video-123?si=first ",
            "https://youtube.com/shorts/video-123?feature=share#comments"
        );

        let links = extract_media_links(Some(text), None, &handlers);

        assert_eq!(links.len(), 1);
        assert_eq!(
            links.first().map(|link| link.original_url.as_str()),
            Some("https://www.youtube.com/shorts/video-123?si=first")
        );
    }

    #[test]
    fn extracts_links_from_captions() {
        let handlers = assert_ok!(create_handlers(&PlatformConfig::default()));

        let links = extract_media_links(
            None,
            Some("A caption with https://x.com/driver/status/123"),
            &handlers,
        );

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
        assert_ne!(
            normalize_cache_key(Platform::Instagram, "https://www.instagram.com/p/post-123"),
            "instagram:post:post-123"
        );
    }

    #[test]
    fn ignores_messages_without_supported_urls() {
        let handlers = assert_ok!(create_handlers(&PlatformConfig::default()));

        let links = extract_media_links(
            Some("No media here"),
            Some("https://example.com/not-supported"),
            &handlers,
        );

        assert!(links.is_empty());
    }
}
