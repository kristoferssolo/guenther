use crate::{
    download::{DownloadResult, platform::cobalt::download_with_cobalt},
    error::Result,
};

/// Download Instagram media with Cobalt.
///
/// # Errors
///
/// Propagates Cobalt API and media download errors.
pub async fn download_instagram(url: String) -> Result<DownloadResult> {
    download_with_cobalt(&url).await
}
