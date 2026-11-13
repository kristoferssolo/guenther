use crate::config::{get_string_from_env, parse_flag};
use std::path::PathBuf;
use tracing::warn;
use url::Url;

/// Where `telegram-bot-api --local` stores files by default, matching the
/// server's `--dir` in `compose.yml`.
const DEFAULT_LOCAL_DIR: &str = "/var/lib/telegram-bot-api";

/// Telegram Bot API settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TelegramConfig {
    /// Bot API server to use instead of the cloud API, such as a proxy or a
    /// self-hosted `telegram-bot-api`.
    pub api_url: Option<Url>,
    /// Data directory of a `telegram-bot-api --local` server at `api_url`,
    /// mounted at the same path. Set only in local mode, which accepts larger
    /// uploads; always `None` without `api_url`.
    pub local_dir: Option<PathBuf>,
}

impl TelegramConfig {
    pub(super) fn from_env() -> Self {
        let api_url = get_string_from_env("TELEGRAM_API_URL").and_then(|raw| {
            Url::parse(raw.trim_end_matches('/'))
                .inspect_err(|err| {
                    warn!(raw = %raw, %err, "TELEGRAM_API_URL is not a valid URL; using the cloud Bot API");
                })
                .ok()
        });
        let local = get_string_from_env("TELEGRAM_API_LOCAL")
            .is_some_and(|raw| parse_flag("TELEGRAM_API_LOCAL", &raw));
        if local && api_url.is_none() {
            warn!("TELEGRAM_API_LOCAL is set without TELEGRAM_API_URL; using cloud upload limits");
        }
        let local_dir = (local && api_url.is_some()).then(|| {
            get_string_from_env("TELEGRAM_API_DIR")
                .unwrap_or_else(|| DEFAULT_LOCAL_DIR.to_owned())
                .into()
        });
        Self { api_url, local_dir }
    }

    /// Whether `api_url` runs `telegram-bot-api --local`.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        self.local_dir.is_some()
    }
}
