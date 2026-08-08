use chrono::{FixedOffset, Offset, Utc};
use std::env;
use tracing::warn;

#[derive(Debug, Clone, Copy)]
pub struct F1Config {
    pub utc_offset: FixedOffset,
}

impl F1Config {
    pub(super) fn from_env() -> Self {
        let utc_offset = match env::var("F1_UTC_OFFSET") {
            Ok(raw) => parse_utc_offset(&raw).unwrap_or_else(|| {
                warn!(raw = %raw, "F1_UTC_OFFSET is set but invalid; expected +3, +03, or +03:00");
                Utc.fix()
            }),
            Err(env::VarError::NotPresent) => Utc.fix(),
            Err(env::VarError::NotUnicode(_)) => {
                warn!("F1_UTC_OFFSET is not valid unicode");
                Utc.fix()
            }
        };

        Self { utc_offset }
    }
}

impl Default for F1Config {
    fn default() -> Self {
        Self {
            utc_offset: Utc.fix(),
        }
    }
}

fn parse_utc_offset(raw: &str) -> Option<FixedOffset> {
    let trimmed = raw.trim();
    let (is_negative, offset) = trimmed.strip_prefix('+').map_or_else(
        || {
            trimmed
                .strip_prefix('-')
                .map_or((false, trimmed), |offset| (true, offset))
        },
        |offset| (false, offset),
    );

    let mut parts = offset.split(':');
    let hours = parts.next().filter(|value| !value.is_empty())?;
    let minutes = parts.next().unwrap_or("0");
    if parts.next().is_some() {
        return None;
    }

    let Ok(hours) = hours.parse::<i32>() else {
        return None;
    };
    let Ok(minutes) = minutes.parse::<i32>() else {
        return None;
    };
    let seconds = hours
        .checked_mul(3_600)?
        .checked_add(minutes.checked_mul(60)?)?;
    let seconds = if is_negative {
        seconds.checked_neg()?
    } else {
        seconds
    };
    FixedOffset::east_opt(seconds)
}
