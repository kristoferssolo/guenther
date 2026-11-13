use crate::{config::Platform, utils::MediaKind};
use std::{
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use strum::Display;

/// In-memory media health counters since startup, shared by every clone.
#[derive(Debug, Clone, Default)]
pub struct MediaStats {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    instagram_failures: AtomicU64,
    tiktok_failures: AtomicU64,
    twitter_failures: AtomicU64,
    youtube_failures: AtomicU64,
    last_instagram_reel: Mutex<Option<LastReel>>,
}

/// What a fresh Instagram reel download produced.
///
/// Without valid account cookies Cobalt falls back to the cover image, so
/// `ImageOnly` is a proxy for expired cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum ReelOutcome {
    Video,
    ImageOnly,
}

impl ReelOutcome {
    pub fn from_kinds(kinds: impl IntoIterator<Item = MediaKind>) -> Self {
        if kinds.into_iter().any(|kind| kind == MediaKind::Video) {
            Self::Video
        } else {
            Self::ImageOnly
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastReel {
    pub outcome: ReelOutcome,
    pub at: Instant,
}

impl MediaStats {
    pub fn record_failure(&self, platform: Platform) {
        self.failure_counter(platform)
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn failures(&self, platform: Platform) -> u64 {
        self.failure_counter(platform).load(Ordering::Relaxed)
    }

    pub fn record_instagram_reel(&self, outcome: ReelOutcome) {
        *self.last_reel_slot() = Some(LastReel {
            outcome,
            at: Instant::now(),
        });
    }

    pub fn last_instagram_reel(&self) -> Option<LastReel> {
        *self.last_reel_slot()
    }

    fn failure_counter(&self, platform: Platform) -> &AtomicU64 {
        match platform {
            Platform::Instagram => &self.inner.instagram_failures,
            Platform::Tiktok => &self.inner.tiktok_failures,
            Platform::Twitter => &self.inner.twitter_failures,
            Platform::Youtube => &self.inner.youtube_failures,
        }
    }

    fn last_reel_slot(&self) -> MutexGuard<'_, Option<LastReel>> {
        self.inner
            .last_instagram_reel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some};
    use rstest::rstest;

    #[test]
    fn failures_count_per_platform_across_clones() {
        let stats = MediaStats::default();
        let clone = stats.clone();

        stats.record_failure(Platform::Instagram);
        clone.record_failure(Platform::Instagram);
        clone.record_failure(Platform::Youtube);

        assert_eq!(stats.failures(Platform::Instagram), 2);
        assert_eq!(stats.failures(Platform::Youtube), 1);
        assert_eq!(stats.failures(Platform::Tiktok), 0);
        assert_eq!(stats.failures(Platform::Twitter), 0);
    }

    #[test]
    fn keeps_only_the_latest_reel_outcome() {
        let stats = MediaStats::default();
        assert_none!(stats.last_instagram_reel());

        stats.record_instagram_reel(ReelOutcome::Video);
        stats.record_instagram_reel(ReelOutcome::ImageOnly);

        let last = assert_some!(stats.last_instagram_reel());
        assert_eq!(last.outcome, ReelOutcome::ImageOnly);
    }

    #[rstest]
    #[case::video(&[MediaKind::Image, MediaKind::Video], ReelOutcome::Video)]
    #[case::cover_only(&[MediaKind::Image], ReelOutcome::ImageOnly)]
    #[case::empty(&[], ReelOutcome::ImageOnly)]
    fn classifies_reel_media(#[case] kinds: &[MediaKind], #[case] expected: ReelOutcome) {
        assert_eq!(ReelOutcome::from_kinds(kinds.iter().copied()), expected);
    }
}
