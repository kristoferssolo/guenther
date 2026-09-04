mod cobalt;
mod f1;
mod platform;

use std::env;
use tracing::warn;

pub use cobalt::CobaltConfig;
pub use f1::F1Config;
pub use platform::{ParsePlatformError, Platform, PlatformConfig};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub chat_id: Option<i64>,
    pub cobalt: CobaltConfig,
    pub platforms: PlatformConfig,
    pub f1: F1Config,
}

impl Config {
    /// Load configuration from environment variables.
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
                warn!("CHAT_ID is not valid Unicode");
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
}

fn get_string_from_env(env_key: &str) -> Option<String> {
    match env::var(env_key) {
        Ok(raw) => match raw.trim() {
            "" => None,
            value => Some(value.to_owned()),
        },
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            warn!(env_key, "Environment variable is not valid Unicode");
            None
        }
    }
}
