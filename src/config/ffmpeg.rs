use crate::config::get_string_from_env;
use std::path::PathBuf;

const DEFAULT_FFMPEG_BIN: &str = "ffmpeg";
const DEFAULT_FFPROBE_BIN: &str = "ffprobe";

/// Executables used to shrink oversized media and to extract LLM caption frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegConfig {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl FfmpegConfig {
    pub(super) fn from_env() -> Self {
        Self {
            ffmpeg: env_path("FFMPEG_BIN", DEFAULT_FFMPEG_BIN),
            ffprobe: env_path("FFPROBE_BIN", DEFAULT_FFPROBE_BIN),
        }
    }
}

impl Default for FfmpegConfig {
    fn default() -> Self {
        Self {
            ffmpeg: PathBuf::from(DEFAULT_FFMPEG_BIN),
            ffprobe: PathBuf::from(DEFAULT_FFPROBE_BIN),
        }
    }
}

fn env_path(key: &str, default: &str) -> PathBuf {
    get_string_from_env(key).map_or_else(|| PathBuf::from(default), PathBuf::from)
}
