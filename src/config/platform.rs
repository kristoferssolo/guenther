use std::{
    collections::HashSet,
    env,
    fmt::{self, Display},
    str::FromStr,
};
use thiserror::Error;
use tracing::warn;

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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("Unknown platform `{0}`")]
pub struct ParsePlatformError(String);

impl PlatformConfig {
    pub(super) fn from_env() -> Self {
        let raw = match env::var("ENABLED_PLATFORMS") {
            Ok(raw) => raw,
            Err(env::VarError::NotPresent) => return Self::default(),
            Err(env::VarError::NotUnicode(_)) => {
                warn!("ENABLED_PLATFORMS is not valid Unicode; enabling all platforms");
                return Self::default();
            }
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

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            enabled: Platform::ALL.into_iter().collect(),
        }
    }
}

impl Platform {
    pub const ALL: [Self; 4] = [Self::Instagram, Self::Tiktok, Self::Twitter, Self::Youtube];
}

impl FromStr for Platform {
    type Err = ParsePlatformError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "instagram" => Ok(Self::Instagram),
            "tiktok" => Ok(Self::Tiktok),
            "twitter" | "x" => Ok(Self::Twitter),
            "youtube" => Ok(Self::Youtube),
            _ => Err(ParsePlatformError(value.to_owned())),
        }
    }
}

impl Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Instagram => "instagram",
            Self::Tiktok => "tiktok",
            Self::Twitter => "twitter",
            Self::Youtube => "youtube",
        })
    }
}
