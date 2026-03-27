use crate::{
    download::media_files::DownloadResult,
    error::{Error, Result},
    utils::{IMAGE_EXTENSIONS, VIDEO_EXTENSIONS},
};
use std::io;
use std::path::{Path, PathBuf};
use std::{ffi::OsStr, fs, process::Stdio};
use tempfile::tempdir;
use tokio::{fs::read_dir, process::Command};
use tracing::{debug, warn};

const FORBIDDEN_EXTENSIONS: &[&str] = &["json", "txt", "log"];

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
pub async fn run_command_in_tempdir(cmd: &str, args: &[&str]) -> Result<DownloadResult> {
    let tmp = tempdir()?;
    run_command_in_dir(tmp, cmd, args).await
}

/// Run a command in the provided temporary directory and collect regular files
/// produced there.
///
/// # Errors
///
/// - `Error::Io` for filesystem / spawn errors (propagated).
/// - `Error::Other` for non-zero exit code (with stderr).
/// - `Error::NoMediaFound` if no files were produced.
pub async fn run_command_in_dir(
    tmp: tempfile::TempDir,
    cmd: &str,
    args: &[&str],
) -> Result<DownloadResult> {
    let cwd = tmp.path().to_path_buf();

    debug!(command = %cmd, cwd = %cwd.display(), args = ?args, "spawning command");

    let output = match Command::new(cmd)
        .current_dir(&cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(Error::other(format!(
                "required executable `{cmd}` was not found on PATH"
            )));
        }
        Err(err) => return Err(err.into()),
    };

    debug!(
        command = %cmd,
        status = ?output.status.code(),
        stdout_bytes = output.stdout.len(),
        stderr_bytes = output.stderr.len(),
        "command finished"
    );

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        debug!(
            command = %cmd,
            cwd = %cwd.display(),
            stderr = %stderr,
            "command failed"
        );
        let err = match cmd {
            "yt-dlp" => Error::ytdlp_failed(stderr),
            _ => Error::Other(format!("{cmd} failed: {stderr}")),
        };
        return Err(err);
    }

    let files = collect_media_files_recursively(&cwd).await?;

    debug!(files = files.len(), "Collected files from tempdir");

    if files.is_empty() {
        let dir_contents = fs::read_dir(&cwd)
            .map(|rd| {
                rd.filter_map(std::result::Result::ok)
                    .map(|e| e.path())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        warn!(?dir_contents, "No media files found in tempdir");
        return Err(Error::NoMediaFound);
    }

    Ok(DownloadResult {
        tempdir: tmp,
        files,
        source_text: None,
    })
}

async fn collect_media_files_recursively(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = stack.pop() {
        debug!(dir = %dir.display(), "scanning download directory");
        let mut rd = read_dir(&dir).await?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            let ty = entry.file_type().await?;

            if ty.is_symlink() {
                debug!(path = %path.display(), "skipping symlink in download directory");
                continue;
            }
            if ty.is_dir() {
                debug!(path = %path.display(), "descending into download subdirectory");
                stack.push(path);
            } else if ty.is_file() && is_potential_media_file(&path) {
                debug!(path = %path.display(), "found candidate media file");
                files.push(path);
            } else if ty.is_file() {
                debug!(path = %path.display(), "skipping non-media file");
            }
        }
    }
    files.sort();
    Ok(files)
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

    VIDEO_EXTENSIONS
        .iter()
        .chain(IMAGE_EXTENSIONS.iter())
        .any(|allowed| allowed.eq_ignore_ascii_case(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_err;
    use tokio::runtime::Builder;

    #[test]
    fn is_potential_media_file_() {
        assert!(is_potential_media_file(Path::new("video.mp4")));
        assert!(is_potential_media_file(Path::new("image.jpg")));
        assert!(!is_potential_media_file(Path::new(".DS_Store")));
        assert!(!is_potential_media_file(Path::new("metadata.json")));
        assert!(!is_potential_media_file(Path::new("download.log")));
    }

    #[test]
    fn missing_executable_returns_clear_error() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create tokio runtime");
        let err = assert_err!(runtime.block_on(run_command_in_tempdir(
            "definitely-not-installed-guenther-test-bin",
            &[],
        )));

        assert_eq!(
            err.to_string(),
            "other: required executable `definitely-not-installed-guenther-test-bin` was not found on PATH"
        );
    }
}
