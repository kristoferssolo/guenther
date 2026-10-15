mod cobalt;
mod f1;
mod platform;

use std::env;
use tracing::warn;

pub use crate::config::cobalt::CobaltConfig;
pub use crate::config::f1::F1Config;
pub use crate::config::platform::{ParsePlatformError, Platform, PlatformConfig};

#[derive(Debug, Clone)]
pub struct Config {
    pub chat_id: Option<i64>,
    pub database_url: String,
    pub cobalt: CobaltConfig,
    pub platforms: PlatformConfig,
    pub f1: F1Config,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let chat_id = get_string_from_env("CHAT_ID").and_then(|raw| {
            raw.parse::<i64>()
                .inspect_err(|_| warn!(raw = %raw, "CHAT_ID is set but invalid; expected i64"))
                .ok()
        });
        Self {
            chat_id,
            database_url: get_string_from_env("DATABASE_URL")
                .unwrap_or_else(|| crate::db::DEFAULT_DATABASE_URL.to_owned()),
            cobalt: CobaltConfig::from_env(),
            platforms: PlatformConfig::from_env(),
            f1: F1Config::from_env(),
        }
    }
}

fn get_string_from_env(env_key: &str) -> Option<String> {
    let raw = env_var(env_key)?;
    match raw.trim() {
        "" => None,
        value => Some(value.to_owned()),
    }
}

fn env_var(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            warn!(key, "Environment variable is not valid Unicode");
            None
        }
    }
}
