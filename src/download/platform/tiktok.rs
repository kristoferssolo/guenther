use crate::{
    config::global_config,
    download::{DownloadResult, platform::run_yt_dlp},
    error::Result,
};

/// Download a Tiktok URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
pub async fn download_tiktok(url: String) -> Result<DownloadResult> {
    let config = global_config();
    run_yt_dlp(&["-t", "mp4"], config.tiktok.cookies_path.as_ref(), &url).await
}
