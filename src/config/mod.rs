mod cobalt;
mod f1;
mod platform;

use crate::error::{Error, Result};
use std::{env, sync::OnceLock};
use tracing::warn;

pub use cobalt::CobaltConfig;
pub use f1::F1Config;
pub use platform::{ParsePlatformError, Platform, PlatformConfig};

static GLOBAL_CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub chat_id: Option<i64>,
    pub cobalt: CobaltConfig,
    pub platforms: PlatformConfig,
    pub f1: F1Config,
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

/// Get global config, lazily using defaults when not explicitly initialized.
#[inline]
#[must_use]
pub fn global_config() -> &'static Config {
    GLOBAL_CONFIG.get_or_init(Config::default)
}

fn get_string_from_env(env_key: &str) -> Option<String> {
    match env::var(env_key) {
        Ok(raw) => match raw.trim() {
            "" => None,
            value => Some(value.to_owned()),
        },
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            warn!(env_key, "env var is not valid unicode");
            None
        }
    }
}
