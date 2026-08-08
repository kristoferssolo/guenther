use crate::error::{Error, Result};
use std::{collections::HashSet, env, fmt::Debug, sync::OnceLock};
use time::UtcOffset;
use tracing::warn;

pub const FAILED_FETCH_MEDIA_MESSAGE: &str = "Failed to fetch media, you foking donkey.";
static GLOBAL_CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub chat_id: Option<i64>,
    pub cobalt: CobaltConfig,
    pub platforms: PlatformConfig,
    pub f1: F1Config,
}

#[derive(Debug, Clone)]
pub struct CobaltConfig {
    pub api_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlatformConfig {
    enabled: HashSet<Platform>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Instagram,
    Tiktok,
    Twitter,
    Youtube,
}

#[derive(Debug, Clone, Copy)]
pub struct F1Config {
    pub utc_offset: UtcOffset,
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
                Some,
            ),
            Err(env::VarError::NotPresent) => None,
            Err(env::VarError::NotUnicode(_)) => {
                warn!("CHAT_ID is not valid unicode");
                None
            }
        };
        Self {
            chat_id,
            cobalt: CobaltConfig::from_env(),
            platforms: PlatformConfig::from_env(),
            f1: F1Config::from_env(),
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

impl CobaltConfig {
    const DEFAULT_API_URL: &'static str = "http://127.0.0.1:9000/";

    fn from_env() -> Self {
        Self {
            api_url: get_string_from_env("COBALT_API_URL")
                .unwrap_or_else(|| Self::DEFAULT_API_URL.to_owned()),
            api_key: get_string_from_env("COBALT_API_KEY"),
        }
    }
}

impl PlatformConfig {
    fn from_env() -> Self {
        let raw = match env::var("ENABLED_PLATFORMS") {
            Ok(raw) => raw,
            Err(env::VarError::NotPresent) => return Self::default(),
            Err(env::VarError::NotUnicode(_)) => {
                warn!("ENABLED_PLATFORMS is not valid unicode; enabling all platforms");
                return Self::default();
            }
        };
        let mut config = Self::none();

        for name in raw
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let platform = match name.to_ascii_lowercase().as_str() {
                "all" => return Self::default(),
                "instagram" => Platform::Instagram,
                "tiktok" => Platform::Tiktok,
                "twitter" | "x" => Platform::Twitter,
                "youtube" => Platform::Youtube,
                unknown => {
                    warn!(platform = unknown, "unknown platform in ENABLED_PLATFORMS");
                    continue;
                }
            };
            config.enabled.insert(platform);
        }

        config
    }

    #[must_use]
    pub fn is_enabled(&self, platform: Platform) -> bool {
        self.enabled.contains(&platform)
    }

    fn none() -> Self {
        Self {
            enabled: HashSet::new(),
        }
    }
}

impl Platform {
    pub const ALL: [Self; 4] = [Self::Instagram, Self::Tiktok, Self::Twitter, Self::Youtube];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Instagram => "instagram",
            Self::Tiktok => "tiktok",
            Self::Twitter => "twitter",
            Self::Youtube => "youtube",
        }
    }
}

impl F1Config {
    fn from_env() -> Self {
        let utc_offset = match env::var("F1_UTC_OFFSET") {
            Ok(raw) => parse_utc_offset(&raw).unwrap_or_else(|| {
                warn!(raw = %raw, "F1_UTC_OFFSET is set but invalid; expected +3, +03, or +03:00");
                UtcOffset::UTC
            }),
            Err(env::VarError::NotPresent) => UtcOffset::UTC,
            Err(env::VarError::NotUnicode(_)) => {
                warn!("F1_UTC_OFFSET is not valid unicode");
                UtcOffset::UTC
            }
        };

        Self { utc_offset }
    }
}

fn get_string_from_env(env_key: &str) -> Option<String> {
    match env::var(env_key) {
        Ok(raw) => match raw.trim() {
            "" => None,
            value => Some(value.to_owned()),
        },
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            warn!(env_key = env_key, "env var is not valid unicode");
            None
        }
    }
}

fn parse_utc_offset(raw: &str) -> Option<UtcOffset> {
    let trimmed = raw.trim();
    let (is_negative, offset) = match trimmed.as_bytes().first().copied() {
        Some(b'+') => (false, &trimmed[1..]),
        Some(b'-') => (true, &trimmed[1..]),
        _ => (false, trimmed),
    };

    let mut parts = offset.split(':');
    let hours = parts.next().filter(|value| !value.is_empty())?;
    let minutes = parts.next().unwrap_or("0");
    if parts.next().is_some() {
        return None;
    }

    let Ok(hours) = hours.parse::<i8>() else {
        return None;
    };
    let Ok(minutes) = minutes.parse::<i8>() else {
        return None;
    };

    let (hours, minutes) = if is_negative {
        (-hours, -minutes)
    } else {
        (hours, minutes)
    };

    UtcOffset::from_hms(hours, minutes, 0).ok()
}

impl Default for CobaltConfig {
    fn default() -> Self {
        Self {
            api_url: Self::DEFAULT_API_URL.to_owned(),
            api_key: None,
        }
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            enabled: Platform::ALL.into_iter().collect(),
        }
    }
}

impl Default for F1Config {
    fn default() -> Self {
        Self {
            utc_offset: UtcOffset::UTC,
        }
    }
}
