use crate::{
    config::global_config,
    download::{DownloadResult, platform::run_yt_dlp},
    error::Result,
};

/// Download a Instagram URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
pub async fn download_instagram(url: String) -> Result<DownloadResult> {
    let config = global_config();
    run_yt_dlp(&["-t", "mp4"], config.instagram.cookies_path.as_ref(), &url).await
}
