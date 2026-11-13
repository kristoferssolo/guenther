//! Health report for the admin `/status` command.

mod stats;

pub use crate::status::stats::{LastReel, MediaStats, ReelOutcome};

use crate::{
    cache::{CacheCount, MediaCache},
    config::Platform,
    handler::MediaHandlers,
};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    time::{Duration, Instant},
};
use tracing::warn;

const COMPILED_FEATURES: [(&str, bool); 3] = [
    ("bingo", cfg!(feature = "bingo")),
    ("llm-captions", cfg!(feature = "llm-captions")),
    ("voice-line-capture", cfg!(feature = "voice-line-capture")),
];

/// Collects the `/status` report from the running bot.
#[derive(Debug)]
pub struct BotStatus {
    started_at: Instant,
    handlers: MediaHandlers,
    cache: MediaCache,
}

impl BotStatus {
    /// Start measuring uptime now.
    pub fn new(handlers: MediaHandlers, cache: MediaCache) -> Self {
        Self {
            started_at: Instant::now(),
            handlers,
            cache,
        }
    }

    /// Probe Cobalt and the media cache and render the report.
    ///
    /// Probe and database failures become report lines instead of errors, so
    /// the command still answers when a dependency is down.
    pub async fn report(&self) -> String {
        let (cobalt, cache) = tokio::join!(
            self.handlers.downloader().cobalt_version(),
            self.cache.platform_counts()
        );
        let cobalt = cobalt.map_err(|err| {
            warn!(%err, "Cobalt status probe failed");
            root_cause(&err)
        });
        let cache = cache.map_err(|err| {
            warn!(%err, "Media cache count failed");
            err.to_string()
        });

        #[cfg(feature = "llm-captions")]
        let llm_model = self.handlers.llm_model();
        #[cfg(not(feature = "llm-captions"))]
        let llm_model = None;

        let now = Instant::now();
        let stats = self.handlers.stats();
        let platforms = self.handlers.enabled_platforms().collect::<Vec<_>>();
        StatusReport {
            version: env!("CARGO_PKG_VERSION"),
            uptime: now.saturating_duration_since(self.started_at),
            features: COMPILED_FEATURES
                .iter()
                .filter_map(|&(name, enabled)| enabled.then_some(name))
                .collect(),
            failures: platforms
                .iter()
                .map(|&platform| (platform, stats.failures(platform)))
                .collect(),
            last_reel: stats
                .last_instagram_reel()
                .map(|last| (last.outcome, now.saturating_duration_since(last.at))),
            platforms,
            cobalt,
            llm_model,
            cache,
        }
        .to_string()
    }
}

/// Plain-text `/status` reply, one fact per line.
#[derive(Debug)]
struct StatusReport<'a> {
    version: &'a str,
    uptime: Duration,
    features: Vec<&'a str>,
    platforms: Vec<Platform>,
    /// Cobalt version, or why the probe failed.
    cobalt: Result<String, String>,
    /// `None` when LLM captions are not built or not configured.
    llm_model: Option<&'a str>,
    cache: Result<Vec<CacheCount>, String>,
    failures: Vec<(Platform, u64)>,
    /// Outcome and age of the latest fresh Instagram reel download.
    last_reel: Option<(ReelOutcome, Duration)>,
}

impl Display for StatusReport<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "version: {}", self.version)?;
        writeln!(f, "uptime: {}", format_duration(self.uptime))?;
        writeln!(f, "features: {}", join_or_none(&self.features))?;
        writeln!(f, "platforms: {}", join_or_none(&self.platforms))?;
        match &self.cobalt {
            Ok(version) => writeln!(f, "cobalt: up {version}")?,
            Err(reason) => writeln!(f, "cobalt: down ({reason})")?,
        }
        writeln!(f, "llm: {}", self.llm_model.unwrap_or("off"))?;
        match &self.cache {
            Ok(counts) if counts.is_empty() => writeln!(f, "cache: empty")?,
            Ok(counts) => {
                for count in counts {
                    let platform = count
                        .platform
                        .map_or_else(|| "other".to_owned(), |platform| platform.to_string());
                    writeln!(
                        f,
                        "cache {platform}: links {}, items {}",
                        count.links, count.items
                    )?;
                }
            }
            Err(reason) => writeln!(f, "cache: error ({reason})")?,
        }
        for (platform, failures) in &self.failures {
            writeln!(f, "failures {platform}: {failures}")?;
        }
        match self.last_reel {
            None => writeln!(f, "last instagram reel: none since start")?,
            Some((outcome, age)) => writeln!(
                f,
                "last instagram reel: {outcome}, {} ago",
                format_duration(age)
            )?,
        }
        Ok(())
    }
}

fn join_or_none<T: Display>(items: &[T]) -> String {
    if items.is_empty() {
        return "none".to_owned();
    }
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format as `45s`, `12m`, `3h 4m`, or `2d 3h 4m`.
fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    if total_secs < 60 {
        return format!("{total_secs}s");
    }
    let days = total_secs / 86_400;
    let hours = total_secs % 86_400 / 3_600;
    let minutes = total_secs % 3_600 / 60;
    match (days, hours) {
        (0, 0) => format!("{minutes}m"),
        (0, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h {minutes}m"),
    }
}

/// The innermost error message, such as `Connection refused` instead of
/// reqwest's generic "error sending request".
fn root_cause(err: &(dyn Error + 'static)) -> String {
    let mut cause = err;
    while let Some(source) = cause.source() {
        cause = source;
    }
    cause.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        comments::Comments,
        config::{CobaltConfig, FfmpegConfig, PlatformConfig},
        db,
        download::platform::Downloader,
        ffmpeg::Ffmpeg,
        handler::{MediaFitter, UploadLimits},
    };
    use claims::assert_ok;
    use rstest::rstest;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    fn report() -> StatusReport<'static> {
        StatusReport {
            version: "0.1.0",
            uptime: Duration::from_secs(93_784),
            features: vec!["bingo", "llm-captions"],
            platforms: vec![Platform::Instagram, Platform::Twitter],
            cobalt: Ok("11.0.2".to_owned()),
            llm_model: Some("z-ai/glm-5.3-flash"),
            cache: Ok(vec![
                CacheCount {
                    platform: Some(Platform::Instagram),
                    links: 12,
                    items: 30,
                },
                CacheCount {
                    platform: None,
                    links: 1,
                    items: 1,
                },
            ]),
            failures: vec![(Platform::Instagram, 2), (Platform::Twitter, 0)],
            last_reel: Some((ReelOutcome::ImageOnly, Duration::from_secs(300))),
        }
    }

    #[test]
    fn formats_one_fact_per_line() {
        assert_eq!(
            report().to_string(),
            "\
version: 0.1.0
uptime: 1d 2h 3m
features: bingo, llm-captions
platforms: instagram, twitter
cobalt: up 11.0.2
llm: z-ai/glm-5.3-flash
cache instagram: links 12, items 30
cache other: links 1, items 1
failures instagram: 2
failures twitter: 0
last instagram reel: image-only, 5m ago
"
        );
    }

    #[test]
    fn formats_degraded_and_empty_states() {
        let report = StatusReport {
            features: Vec::new(),
            platforms: Vec::new(),
            cobalt: Err("Connection refused".to_owned()),
            llm_model: None,
            cache: Err("database is locked".to_owned()),
            failures: Vec::new(),
            last_reel: None,
            ..report()
        };

        assert_eq!(
            report.to_string(),
            "\
version: 0.1.0
uptime: 1d 2h 3m
features: none
platforms: none
cobalt: down (Connection refused)
llm: off
cache: error (database is locked)
last instagram reel: none since start
"
        );
    }

    #[test]
    fn formats_empty_cache() {
        let text = StatusReport {
            cache: Ok(Vec::new()),
            ..report()
        }
        .to_string();

        assert!(text.lines().any(|line| line == "cache: empty"), "{text}");
    }

    #[rstest]
    #[case::seconds(45, "45s")]
    #[case::minutes(60, "1m")]
    #[case::hours(3_660, "1h 1m")]
    #[case::days(172_800, "2d 0h 0m")]
    fn formats_durations(#[case] secs: u64, #[case] expected: &str) {
        assert_eq!(format_duration(Duration::from_secs(secs)), expected);
    }

    #[tokio::test]
    async fn reports_down_cobalt_instead_of_failing() {
        let listener = assert_ok!(TcpListener::bind("127.0.0.1:0").await);
        let address = assert_ok!(listener.local_addr());
        drop(listener);
        let downloader = assert_ok!(Downloader::new(CobaltConfig {
            api_url: format!("http://{address}/"),
            api_key: None,
        }));
        let handlers = assert_ok!(MediaHandlers::new(
            &PlatformConfig::default(),
            downloader,
            MediaFitter::new(Ffmpeg::new(&FfmpegConfig::default()), UploadLimits::CLOUD),
            Arc::new(Comments::default()),
        ));
        let cache = MediaCache::new(assert_ok!(db::connect("sqlite::memory:").await));

        let text = BotStatus::new(handlers, cache).report().await;

        assert!(
            text.lines().any(|line| line.starts_with("cobalt: down (")),
            "{text}"
        );
        assert!(text.lines().any(|line| line == "cache: empty"), "{text}");
    }
}
