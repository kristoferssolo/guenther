use crate::{
    download::{DownloadResult, platform::cobalt::download_with_cobalt},
    error::Result,
};
use url::Url;

/// Download Instagram media with Cobalt.
///
/// # Errors
///
/// Propagates Cobalt API and media download errors.
pub async fn download_instagram(url: String) -> Result<DownloadResult> {
    download_with_cobalt(&url).await
}

/// Remove URL details that do not identify a different Instagram post.
pub fn normalized_cache_key(source_url: &str) -> String {
    let Ok(mut url) = Url::parse(source_url) else {
        return source_url.to_owned();
    };

    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_owned();
    if path != url.path() {
        url.set_path(&path);
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_cache_key_omits_query_fragment_and_trailing_slash() {
        assert_eq!(
            normalized_cache_key("https://www.instagram.com/p/ABC123/?utm_source=chat#media"),
            "https://www.instagram.com/p/ABC123"
        );
    }
}
