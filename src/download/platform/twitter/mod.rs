mod metadata;
mod syndication;

use crate::{
    download::{DownloadResult, platform::cobalt::download_with_cobalt},
    error::Result,
};
use tracing::warn;

/// Download a Twitter URL.
///
/// Uses Cobalt for media, fetches post text from the public syndication
/// endpoint, and falls back to that endpoint for image-only posts.
///
/// # Errors
///
/// Returns any download, parsing, or network error encountered while fetching
/// media via Cobalt or the syndication fallback.
pub async fn download_twitter(url: String) -> Result<DownloadResult> {
    match download_with_cobalt(&url).await {
        Ok(mut result) => {
            result.source_text = match syndication::fetch_tweet_text(&url).await {
                Ok(text) => text,
                Err(err) => {
                    warn!(url = %url, %err, "Could not fetch post text from Twitter syndication");
                    None
                }
            };
            Ok(result)
        }
        Err(cobalt_error) => match syndication::download_tweet_images(&url).await {
            Ok(result) => {
                warn!(url = %url, %cobalt_error, "Cobalt could not fetch Twitter media; used image fallback");
                Ok(result)
            }
            Err(fallback_error) => {
                warn!(url = %url, %fallback_error, "Twitter image fallback was not applicable");
                Err(cobalt_error)
            }
        },
    }
}
