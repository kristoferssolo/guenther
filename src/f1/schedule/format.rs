use crate::error::{Error, Result};
use chrono::{DateTime, FixedOffset, TimeDelta, Utc};

/// Display format for session start times, e.g. `Sun, 28 Jun 16:00`.
const DISPLAY_FORMAT: &str = "%a, %d %b %H:%M";

/// Joins the API's split `date` and `time` fields into a single timestamp.
pub(super) fn parse_session_time(date: &str, time: &str) -> Result<DateTime<Utc>> {
    let raw = format!("{date}T{time}");
    DateTime::parse_from_rfc3339(&raw)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|source| Error::ParseF1SessionTime { raw, source })
}

/// Renders a session start in the viewer's offset.
pub(super) fn format_start(start: DateTime<Utc>, offset: FixedOffset) -> String {
    start
        .with_timezone(&offset)
        .format(DISPLAY_FORMAT)
        .to_string()
}

/// Formats a duration as `"2d 4h 30m"`, or `"under a minute"` below 60 seconds.
pub(super) fn format_duration(duration: TimeDelta) -> String {
    if duration.num_seconds() < 60 {
        return "under a minute".to_string();
    }

    let days = duration.num_days();
    let hours = duration.num_hours() % 24;
    let minutes = duration.num_minutes() % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }

    parts.join(" ")
}

/// Formats a UTC offset as `"+3"`, or `"+5:30"` when it is not a whole hour.
pub(super) fn format_offset(offset: FixedOffset) -> String {
    let total_minutes = offset.local_minus_utc() / 60;
    let sign = if total_minutes >= 0 { '+' } else { '-' };
    let absolute_minutes = total_minutes.unsigned_abs();
    let hours = absolute_minutes / 60;
    let minutes = absolute_minutes % 60;

    if minutes == 0 {
        format!("{sign}{hours}")
    } else {
        format!("{sign}{hours}:{minutes:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_ok, assert_some};

    #[test]
    fn format_start_applies_positive_offset() {
        let offset = assert_some!(FixedOffset::east_opt(3 * 3_600));
        let start = assert_ok!(parse_session_time("2026-06-28", "13:00:00Z"));

        assert_eq!(format_start(start, offset), "Sun, 28 Jun 16:00");
    }

    #[test]
    fn format_start_handles_date_rollover() {
        let offset = assert_some!(FixedOffset::west_opt(3 * 3_600));
        let start = assert_ok!(parse_session_time("2026-06-28", "01:00:00Z"));

        assert_eq!(format_start(start, offset), "Sat, 27 Jun 22:00");
    }

    #[test]
    fn format_offset_omits_zero_minutes() {
        let offset = assert_some!(FixedOffset::east_opt(3 * 3_600));

        assert_eq!(format_offset(offset), "+3");
    }

    #[test]
    fn format_duration_multi_day() {
        let duration = assert_some!(TimeDelta::try_seconds(2 * 86_400 + 4 * 3_600 + 30 * 60));

        assert_eq!(format_duration(duration), "2d 4h 30m");
    }

    #[test]
    fn format_duration_hours_only() {
        let duration = assert_some!(TimeDelta::try_seconds(4 * 3_600 + 30 * 60));

        assert_eq!(format_duration(duration), "4h 30m");
    }

    #[test]
    fn format_duration_minutes_only() {
        let duration = assert_some!(TimeDelta::try_minutes(45));

        assert_eq!(format_duration(duration), "45m");
    }

    #[test]
    fn format_duration_under_a_minute() {
        let duration = assert_some!(TimeDelta::try_seconds(45));

        assert_eq!(format_duration(duration), "under a minute");
    }
}
