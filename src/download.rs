use crate::config::global_config;
use crate::{
    error::{Error, Result},
    utils::{
        IMAGE_EXSTENSIONS, MediaKind, VIDEO_EXSTENSIONS, detect_media_kind_async,
        send_media_from_path,
    },
};
use futures::{StreamExt, stream};
use std::{
    cmp::min,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
use teloxide::{Bot, types::ChatId};
use tempfile::{TempDir, tempdir};
use tokio::{fs::read_dir, process::Command};
use tracing::{debug, warn};

const FORBIDDEN_EXTENSIONS: &[&str] = &["json", "txt", "log"];

/// `TempDir` guard + downloaded files. Keep this value alive until you're
/// done sending files so the temporary directory is not deleted.
#[derive(Debug)]
pub struct DownloadResult {
    pub tempdir: TempDir,
    pub files: Vec<PathBuf>,
}

/// Run a command in a freshly created temporary directory and collect
/// regular files produced there.
///
/// # Arguments
///
/// `cmd` is the command name (e.g. "yt-dlp").
/// `args` are the command arguments (owned Strings so callers can build dynamic args).
///
/// # Errors
///
/// - `Error::Io` for filesystem / spawn errors (propagated).
/// - `Error::Other` for non-zero exit code (with stderr).
/// - `Error::NoMediaFound` if no files were produced.
#[allow(clippy::similar_names)]
async fn run_command_in_tempdir(cmd: &str, args: &[&str]) -> Result<DownloadResult> {
    let tmp = tempdir()?;
    let cwd = tmp.path().to_path_buf();

    let output = Command::new(cmd)
        .current_dir(&cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let err = match cmd {
            "yt-dlp" => Error::ytdlp_failed(stderr),
            _ => Error::Other(format!("{cmd} failed: {stderr}")),
        };
        return Err(err);
    }

    // Collect files produced in tempdir (async)
    let mut rd = read_dir(&cwd).await?;
    let mut files = Vec::new();
    while let Some(entry) = rd.next_entry().await? {
        let path = entry.path();
        // Filter out non-media files (logs, metadata, etc.)
        if is_potential_media_file(&path) {
            files.push(path);
        }
    }

    debug!(files = files.len(), "Collected files from tempdir");

    if files.is_empty() {
        let dir_contents = fs::read_dir(&cwd)
            .map(|rd| {
                rd.filter_map(std::result::Result::ok)
                    .map(|e| e.path())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        warn!(dir_contents = ?dir_contents, "No media files found in tempdir");
        return Err(Error::NoMediaFound);
    }

    files.sort();

    Ok(DownloadResult {
        tempdir: tmp,
        files,
    })
}

/// Download a Instagram URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
#[cfg(feature = "instagram")]
pub async fn download_instagram(url: String) -> Result<DownloadResult> {
    let config = global_config();
    run_yt_dlp(&["-t", "mp4"], config.instagram.cookies_path.as_ref(), &url).await
}

/// Download a Tiktok URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
#[cfg(feature = "tiktok")]
pub async fn download_tiktok(url: String) -> Result<DownloadResult> {
    let config = global_config();
    run_yt_dlp(&["-t", "mp4"], config.tiktok.cookies_path.as_ref(), &url).await
}

/// Download a Twitter URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
#[cfg(feature = "twitter")]
pub async fn download_twitter(url: String) -> Result<DownloadResult> {
    let config = global_config();
    run_yt_dlp(&["-t", "mp4"], config.twitter.cookies_path.as_ref(), &url).await
}

/// Download a URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
#[cfg(feature = "youtube")]
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

/// Post-process a `DownloadResult`.
///
/// Detect media kinds (async), prefer video, then image, then call `send_media_from_path`.
/// Keeps the tempdir alive while sending because `DownloadResult` is passed by value.
///
/// # Errors
///
/// - Propagates `send_media_from_path` errors or returns NoMediaFound/UnknownMediaKind.
pub async fn process_download_result(
    bot: &Bot,
    chat_id: ChatId,
    mut dr: DownloadResult,
) -> Result<()> {
    debug!(files = dr.files.len(), "Processing download result");

    if dr.files.is_empty() {
        return Err(Error::NoMediaFound);
    }

    // Detect kinds and validate files in parallel
    let concurrency = min(8, dr.files.len());
    let results = stream::iter(dr.files.drain(..).map(|path| async move {
        // Check file metadata asynchronously
        let Ok(meta) = tokio::fs::metadata(&path).await else {
            return None;
        };
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }

        let kind = detect_media_kind_async(&path).await;
        match kind {
            MediaKind::Unknown => None,
            k => Some((path, k)),
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;

    let mut media_items = results.into_iter().flatten().collect::<Vec<_>>();

    if media_items.is_empty() {
        return Err(Error::NoMediaFound);
    }

    // deterministic ordering
    media_items.sort_by_key(|(_, k)| match k {
        MediaKind::Video => 0,
        MediaKind::Image => 1,
        MediaKind::Unknown => 2,
    });

    debug!(media_items = media_items.len(), "Sending media to chat");

    if let Some((path, kind)) = media_items.first() {
        return send_media_from_path(bot, chat_id, path.clone(), *kind).await;
    }

    Err(Error::NoMediaFound)
}

/// Filter function to determine if a file is potentially media based on name/extension.
fn is_potential_media_file(path: &Path) -> bool {
    if let Some(filename) = path.file_name().and_then(OsStr::to_str) {
        // Skip common non-media files
        if filename.starts_with('.') || filename.to_lowercase().contains("metadata") {
            return false;
        }
    }

    let ext = match path.extension().and_then(OsStr::to_str) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };

    if FORBIDDEN_EXTENSIONS
        .iter()
        .any(|forbidden| forbidden.eq_ignore_ascii_case(&ext))
    {
        return false;
    }

    VIDEO_EXSTENSIONS
        .iter()
        .chain(IMAGE_EXSTENSIONS.iter())
        .any(|allowed| allowed.eq_ignore_ascii_case(&ext))
}

async fn run_yt_dlp(
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

    debug!(args = ?args, "downloading content");
    run_command_in_tempdir("yt-dlp", &args).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_potential_media_file_() {
        assert!(is_potential_media_file(Path::new("video.mp4")));
        assert!(is_potential_media_file(Path::new("image.jpg")));
        assert!(!is_potential_media_file(Path::new(".DS_Store")));
        assert!(!is_potential_media_file(Path::new("metadata.json")));
        assert!(!is_potential_media_file(Path::new("download.log")));
    }
}
