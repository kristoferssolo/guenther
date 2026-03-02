use crate::error::{Error, Result};
use std::{env, fmt::Debug, path::PathBuf, sync::OnceLock};
use teloxide::types::ChatId;
use tracing::warn;

pub const FAILED_FETCH_MEDIA_MESSAGE: &str = "Failed to fetch media, you foking donkey.";
static GLOBAL_CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub chat_id: Option<ChatId>,
    pub youtube: YoutubeConfig,
    pub instagram: InstagramConfig,
    pub tiktok: TiktokConfig,
    pub twitter: TwitterConfig,
}

#[derive(Debug, Clone)]
pub struct YoutubeConfig {
    pub cookies_path: Option<PathBuf>,
    pub postprocessor_args: String,
}

#[derive(Debug, Clone, Default)]
pub struct InstagramConfig {
    pub cookies_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct TiktokConfig {
    pub cookies_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct TwitterConfig {
    pub cookies_path: Option<PathBuf>,
}

impl Config {
    /// Load configuration from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let chat_id = match env::var("CHAT_ID") {
            Ok(raw) => raw.parse::<i64>().map_or_else(
                |_| {
                    warn!(raw = %raw, "CHAT_ID is set but invalid; expected i64");
                    None
                },
                |id| Some(ChatId(id)),
            ),
            Err(env::VarError::NotPresent) => None,
            Err(env::VarError::NotUnicode(_)) => {
                warn!("CHAT_ID is not valid unicode");
                None
            }
        };
        Self {
            chat_id,
            youtube: YoutubeConfig::from_env(),
            instagram: InstagramConfig::from_env(),
            tiktok: TiktokConfig::from_env(),
            twitter: TwitterConfig::from_env(),
        }
    }

    /// Initialize the global config (call once at startup).
    ///
    /// # Errors
    ///
    /// Returns error if config is already initialized.
    pub fn init(self) -> Result<()> {
        GLOBAL_CONFIG
            .set(self)
            .map_err(|_| Error::other("config already initialized"))
    }
}
/// Get global config (initialized by `Config::init(self)`).
///
/// # Panics
///
/// Panics if config has not been initialized.
#[inline]
#[must_use]
pub fn global_config() -> &'static Config {
    GLOBAL_CONFIG.get().expect("config not initialized")
}

impl YoutubeConfig {
    const DEFAULT_POSTPROCESSOR_ARGS: &'static str = "ffmpeg:-vf setsar=1 -c:v libx264 -crf 20 -preset veryfast -c:a aac -b:a 128k -movflags +faststart";

    fn from_env() -> Self {
        Self {
            cookies_path: get_path_from_env("YOUTUBE_SESSION_COOKIE_PATH"),
            postprocessor_args: env::var("YOUTUBE_POSTPROCESSOR_ARGS")
                .unwrap_or_else(|_| Self::DEFAULT_POSTPROCESSOR_ARGS.to_string()),
        }
    }
}

impl InstagramConfig {
    fn from_env() -> Self {
        Self {
            cookies_path: get_path_from_env("IG_SESSION_COOKIE_PATH"),
        }
    }
}

impl TiktokConfig {
    fn from_env() -> Self {
        Self {
            cookies_path: get_path_from_env("TIKTOK_SESSION_COOKIE_PATH"),
        }
    }
}

impl TwitterConfig {
    fn from_env() -> Self {
        Self {
            cookies_path: get_path_from_env("TWITTER_SESSION_COOKIE_PATH"),
        }
    }
}

fn get_path_from_env(env_key: &str) -> Option<PathBuf> {
    match env::var(env_key) {
        Ok(raw) => {
            let path = PathBuf::from(&raw);
            if path.is_file() {
                Some(path)
            } else {
                warn!(env_key = env_key, path = %path.display(), "cookie path is set but not a file");
                None
            }
        }
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            warn!(env_key = env_key, "env var is not valid unicode");
            None
        }
    }
}

impl Default for YoutubeConfig {
    fn default() -> Self {
        Self {
            cookies_path: None,
            postprocessor_args: Self::DEFAULT_POSTPROCESSOR_ARGS.into(),
        }
    }
}
