use crate::config::get_string_from_env;
use std::{fmt, path::PathBuf, time::Duration};
use tracing::warn;

const DEFAULT_FFMPEG_BIN: &str = "ffmpeg";
const DEFAULT_FFPROBE_BIN: &str = "ffprobe";
const DEFAULT_CAPTION_WAIT: Duration = Duration::from_secs(8);

/// `OpenRouter` settings for LLM-written media captions.
#[derive(Clone)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub model: String,
    /// Send videos as video input instead of extracted frames. Needs a model
    /// that accepts video.
    pub video_input: bool,
    /// How long media waits for the LLM caption before going out with a
    /// static caption that is edited later.
    pub caption_wait: Duration,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl OpenRouterConfig {
    /// Read the settings, or `None` unless both `OPENROUTER_API_KEY` and
    /// `OPENROUTER_MODEL` are set.
    pub(super) fn from_env() -> Option<Self> {
        let (api_key, model) = match (
            get_string_from_env("OPENROUTER_API_KEY"),
            get_string_from_env("OPENROUTER_MODEL"),
        ) {
            (Some(api_key), Some(model)) => (api_key, model),
            (None, None) => return None,
            (Some(_), None) => {
                warn!(
                    "OPENROUTER_API_KEY is set without OPENROUTER_MODEL; LLM captions are disabled"
                );
                return None;
            }
            (None, Some(_)) => {
                warn!(
                    "OPENROUTER_MODEL is set without OPENROUTER_API_KEY; LLM captions are disabled"
                );
                return None;
            }
        };

        Some(Self {
            api_key,
            model,
            video_input: get_string_from_env("OPENROUTER_VIDEO_INPUT")
                .is_some_and(|raw| parse_flag("OPENROUTER_VIDEO_INPUT", &raw)),
            caption_wait: get_string_from_env("OPENROUTER_CAPTION_WAIT_SECS")
                .map_or(DEFAULT_CAPTION_WAIT, |raw| parse_wait(&raw)),
            ffmpeg: env_path("FFMPEG_BIN", DEFAULT_FFMPEG_BIN),
            ffprobe: env_path("FFPROBE_BIN", DEFAULT_FFPROBE_BIN),
        })
    }
}

impl fmt::Debug for OpenRouterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenRouterConfig")
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("video_input", &self.video_input)
            .field("caption_wait", &self.caption_wait)
            .field("ffmpeg", &self.ffmpeg)
            .field("ffprobe", &self.ffprobe)
            .finish()
    }
}

fn parse_flag(key: &str, raw: &str) -> bool {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => {
            warn!(
                key,
                raw, "Expected a boolean such as true or false; treating as false"
            );
            false
        }
    }
}

fn parse_wait(raw: &str) -> Duration {
    raw.parse().map_or_else(
        |_| {
            warn!(
                raw,
                default_secs = DEFAULT_CAPTION_WAIT.as_secs(),
                "OPENROUTER_CAPTION_WAIT_SECS is not a whole number of seconds; using the default"
            );
            DEFAULT_CAPTION_WAIT
        },
        Duration::from_secs,
    )
}

fn env_path(key: &str, default: &str) -> PathBuf {
    get_string_from_env(key).map_or_else(|| PathBuf::from(default), PathBuf::from)
}
