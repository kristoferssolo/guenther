//! Thin wrappers around the configured `ffmpeg` and `ffprobe` executables.

use crate::config::FfmpegConfig;
use std::{ffi::OsStr, io, path::Path, process::Output, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{process::Command, time::timeout};
use tracing::warn;

/// Reading a container header is quick; a probe this slow is stuck.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("Failed to run ffmpeg: {0}")]
    Spawn(#[source] io::Error),

    #[error("ffmpeg failed: {0}")]
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    ffmpeg: Arc<Path>,
    ffprobe: Arc<Path>,
}

impl Ffmpeg {
    pub fn new(config: &FfmpegConfig) -> Self {
        Self {
            ffmpeg: config.ffmpeg.as_path().into(),
            ffprobe: config.ffprobe.as_path().into(),
        }
    }

    /// Run ffmpeg quietly with `args`. Dropping the future kills the process,
    /// so callers can bound it with a timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if ffmpeg cannot be started or exits unsuccessfully.
    pub async fn run<I, S>(&self, args: I) -> Result<(), FfmpegError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new(&*self.ffmpeg)
            .args(["-nostdin", "-v", "error", "-y"])
            .args(args)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(FfmpegError::Spawn)?;
        if output.status.success() {
            return Ok(());
        }
        Err(FfmpegError::Failed(stderr(&output)))
    }

    /// Container duration in seconds, or `None` when ffprobe cannot read it
    /// within 30 seconds.
    pub async fn probe_duration(&self, input: &Path) -> Option<f64> {
        let probe = Command::new(&*self.ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(input)
            .kill_on_drop(true)
            .output();
        let Ok(output) = timeout(PROBE_TIMEOUT, probe).await else {
            warn!(timeout = ?PROBE_TIMEOUT, "ffprobe did not read the duration in time");
            return None;
        };
        let output = output
            .inspect_err(|err| warn!(%err, "Failed to run ffprobe"))
            .ok()?;
        if !output.status.success() {
            warn!(stderr = %stderr(&output), "ffprobe could not read the duration");
            return None;
        }
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}
