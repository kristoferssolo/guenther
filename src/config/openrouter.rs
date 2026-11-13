use crate::config::{get_string_from_env, parse_flag};
use std::{fmt, time::Duration};
use tracing::warn;

const DEFAULT_CAPTION_WAIT: Duration = Duration::from_secs(8);
const DEFAULT_REPLY_COOLDOWN: Duration = Duration::from_secs(30);

/// `OpenRouter` settings for LLM-written media captions and replies.
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
    /// Minimum time between two in-character replies in one chat.
    pub reply_cooldown: Duration,
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
            caption_wait: env_secs("OPENROUTER_CAPTION_WAIT_SECS", DEFAULT_CAPTION_WAIT),
            reply_cooldown: env_secs("OPENROUTER_REPLY_COOLDOWN_SECS", DEFAULT_REPLY_COOLDOWN),
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
            .field("reply_cooldown", &self.reply_cooldown)
            .finish()
    }
}

fn env_secs(key: &str, default: Duration) -> Duration {
    let Some(raw) = get_string_from_env(key) else {
        return default;
    };
    raw.parse().map_or_else(
        |_| {
            warn!(
                key,
                raw,
                default_secs = default.as_secs(),
                "Expected a whole number of seconds; using the default"
            );
            default
        },
        Duration::from_secs,
    )
}
