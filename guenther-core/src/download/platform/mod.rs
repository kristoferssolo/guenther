use crate::{
    download::{
        DownloadResult,
        runner::{run_command_in_dir, run_command_in_tempdir},
    },
    error::Result,
};
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::fs;
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
    let mut args = base_args.to_vec();

    if let Some(path) = cookies_path {
        let cookies_tempdir = tempdir()?;
        let staged_cookies_path = cookies_tempdir.path().join(cookie_filename(path));
        fs::copy(path, &staged_cookies_path).await?;
        let cookies_path_str = staged_cookies_path.to_string_lossy().into_owned();
        args.extend(["--cookies", &cookies_path_str]);
        args.push(url);

        debug!(
            url = %url,
            has_cookies = true,
            cookies_path = %path.display(),
            staged_cookies_path = %staged_cookies_path.display(),
            args = ?args,
            "starting yt-dlp download"
        );

        return run_command_in_dir(cookies_tempdir, "yt-dlp", &args).await;
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

fn cookie_filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cookies.txt")
}
