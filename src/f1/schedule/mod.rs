mod format;
mod race;

use crate::error::{Error, Result};
use chrono::{FixedOffset, Utc};
use format::{format_duration, format_start};
use race::{JolpicaResponse, Race, SessionKind, next_session};
use reqwest::Client;

const NEXT_RACE_URL: &str = "https://api.jolpi.ca/ergast/f1/current/next.json";

/// Which sessions of the upcoming race weekend to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleView {
    Weekend,
    Qualifying,
    Race,
}

impl ScheduleView {
    const fn includes(self, kind: SessionKind) -> bool {
        match self {
            Self::Weekend => true,
            Self::Qualifying => matches!(
                kind,
                SessionKind::SprintQualifying | SessionKind::Qualifying
            ),
            Self::Race => matches!(kind, SessionKind::Sprint | SessionKind::Race),
        }
    }
}

/// Fetch and format the next F1 race schedule.
///
/// # Errors
///
/// Returns an error if the API request fails, no race is available, or the API returns invalid
/// session date/time data.
pub(super) async fn next_race_message(
    client: &Client,
    view: ScheduleView,
    offset: FixedOffset,
) -> Result<String> {
    let race = next_race(client).await?;

    let lines: Vec<String> = race
        .sessions()?
        .into_iter()
        .filter(|&(kind, _)| view.includes(kind))
        .map(|(kind, start)| {
            format!(
                "{} {}: {}",
                kind.emoji(),
                kind.name(),
                format_start(start, offset)
            )
        })
        .collect();

    if lines.is_empty() {
        return Err(Error::MissingF1Sessions);
    }

    Ok(format!("{}\n\n{}", race.header(offset), lines.join("\n")))
}

/// Fetch and format a countdown to the next F1 session of the upcoming race weekend.
///
/// # Errors
///
/// Returns an error if the API request fails, no race is available, the API returns invalid
/// session date/time data, or every session of the weekend has already started.
pub(super) async fn countdown_message(client: &Client, offset: FixedOffset) -> Result<String> {
    let race = next_race(client).await?;
    let now = Utc::now();

    let sessions = race.sessions()?;
    let Some((kind, start)) = next_session(&sessions, now) else {
        return Err(Error::MissingF1Sessions);
    };

    Ok(format!(
        "{}\n\n{} Next session: {} – in {}\n🕒 Starts: {}",
        race.header(offset),
        kind.emoji(),
        kind.name(),
        format_duration(start.signed_duration_since(now)),
        format_start(start, offset),
    ))
}

async fn next_race(client: &Client) -> Result<Race> {
    let response = client
        .get(NEXT_RACE_URL)
        .send()
        .await
        .map_err(Error::FetchF1Schedule)?
        .error_for_status()
        .map_err(Error::FetchF1Schedule)?
        .json::<JolpicaResponse>()
        .await
        .map_err(Error::DecodeF1Schedule)?;

    response
        .mr_data
        .race_table
        .races
        .into_iter()
        .next()
        .ok_or(Error::MissingF1Race)
}
