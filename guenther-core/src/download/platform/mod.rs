use crate::{
    download::{DownloadResult, runner::run_command_in_tempdir},
    error::Result,
};
use std::path::PathBuf;
use tracing::debug;

#[cfg(feature = "instagram")]
pub mod instagram;
#[cfg(feature = "tiktok")]
pub mod tiktok;
#[cfg(feature = "twitter")]
pub mod twitter;
#[cfg(feature = "youtube")]
pub mod youtube;

/// Run `yt-dlp` with shared platform-specific arguments.
///
/// # Errors
///
/// Propagates command execution and media collection failures from
/// `run_command_in_tempdir`.
pub async fn run_yt_dlp(
    base_args: &[&str],
    cookies_path: Option<&PathBuf>,
    url: &str,
) -> Result<DownloadResult> {
    let cookies_path_str;
    let mut args = base_args.to_vec();

    if let Some(path) = cookies_path {
        cookies_path_str = path.to_string_lossy();
        args.extend(["--cookies", &cookies_path_str]);
    }
    args.push(url);

    debug!(
        url = %url,
        has_cookies = cookies_path.is_some(),
        cookies_path = cookies_path.map(|path| path.display().to_string()),
        args = ?args,
        "starting yt-dlp download"
    );
    run_command_in_tempdir("yt-dlp", &args).await
}
