use crate::{
    config::global_config,
    download::{DownloadResult, platform::run_yt_dlp},
    error::Result,
};

/// Download a URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
pub async fn download_youtube(url: String) -> Result<DownloadResult> {
    let config = global_config();
    let mut args = vec![
        "--no-playlist",
        "-f",
        "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best",
        "--merge-output-format",
        "mp4",
    ];
    if !config.youtube.postprocessor_args.is_empty() {
        args.extend(["--postprocessor-args", &config.youtube.postprocessor_args]);
    }
    run_yt_dlp(&args, config.youtube.cookies_path.as_ref(), &url).await
}
