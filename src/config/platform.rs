use crate::config::env_var;
use std::collections::HashSet;
use strum::{Display, EnumString, VariantArray};
use thiserror::Error;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct PlatformConfig {
    enabled: HashSet<Platform>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, VariantArray)]
#[strum(
    serialize_all = "lowercase",
    ascii_case_insensitive,
    parse_err_ty = ParsePlatformError,
    parse_err_fn = ParsePlatformError::new
)]
pub enum Platform {
    Instagram,
    Tiktok,
    #[strum(to_string = "twitter", serialize = "x")]
    Twitter,
    Youtube,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Unknown platform `{0}`")]
pub struct ParsePlatformError(String);

impl ParsePlatformError {
    fn new(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl PlatformConfig {
    pub(super) fn from_env() -> Self {
        let Some(raw) = env_var("ENABLED_PLATFORMS") else {
            return Self::default();
        };
        let mut config = Self::none();

        for name in raw
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if name.eq_ignore_ascii_case("all") {
                return Self::default();
            }
            let platform = match name.parse() {
                Ok(platform) => platform,
                Err(error) => {
                    warn!(%error, "Unknown platform in ENABLED_PLATFORMS");
                    continue;
                }
            };
            config.enabled.insert(platform);
        }

        config
    }

    pub fn is_enabled(&self, platform: Platform) -> bool {
        self.enabled.contains(&platform)
    }

    fn none() -> Self {
        Self {
            enabled: HashSet::new(),
        }
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            enabled: Platform::VARIANTS.iter().copied().collect(),
        }
    }
}
