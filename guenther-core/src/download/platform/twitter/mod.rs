mod metadata;
mod syndication;

use crate::{
    config::global_config,
    download::{DownloadResult, platform::run_yt_dlp},
    error::{Error, Result},
};
use tracing::warn;

/// Download a Twitter URL.
///
/// Uses `yt-dlp` for the normal path and falls back to the public syndication
/// endpoint for image-only tweets that `yt-dlp` rejects as having no video.
///
/// # Errors
///
/// Returns any download, parsing, or network error encountered while fetching
/// media via `yt-dlp` or the syndication fallback.
pub async fn download_twitter(url: String) -> Result<DownloadResult> {
    let config = global_config();

    match run_yt_dlp(
        &["-t", "mp4", "--write-info-json"],
        config.twitter.cookies_path.as_ref(),
        &url,
    )
    .await
    {
        Ok(mut result) => {
            result.source_text = metadata::extract_post_text(result.tempdir.path()).await;
            Ok(result)
        }
        Err(err) if is_image_tweet_fallback_case(&err) => {
            warn!(url = %url, %err, "yt-dlp could not fetch twitter media; falling back to image downloader");
            syndication::download_tweet_images(&url).await
        }
        Err(err) => Err(err),
    }
}

fn is_image_tweet_fallback_case(err: &Error) -> bool {
    matches!(err, Error::YTDLPFailed(message) if message.contains("No video could be found in this tweet"))
}
