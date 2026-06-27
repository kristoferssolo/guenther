use crate::error::{Error, Result};
use serde::Deserialize;
use time::{
    OffsetDateTime, UtcOffset,
    format_description::{FormatItem, well_known::Rfc3339},
    macros::format_description,
};

const NEXT_RACE_URL: &str = "https://api.jolpi.ca/ergast/f1/current/next.json";
const DISPLAY_FORMAT: &[FormatItem<'_>] =
    format_description!("[weekday repr:short], [day] [month repr:short] [hour]:[minute]");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleView {
    Weekend,
    Qualifying,
    Race,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JolpicaResponse {
    mr_data: MrData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct MrData {
    race_table: RaceTable,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RaceTable {
    races: Vec<Race>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Race {
    #[serde(rename = "raceName")]
    name: String,
    circuit: Circuit,
    date: String,
    time: String,
    first_practice: Option<Session>,
    second_practice: Option<Session>,
    third_practice: Option<Session>,
    sprint: Option<Session>,
    sprint_qualifying: Option<Session>,
    qualifying: Option<Session>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Circuit {
    #[serde(rename = "circuitName")]
    circuit_name: String,
    location: Location,
}

#[derive(Debug, Deserialize)]
struct Location {
    locality: String,
    country: String,
}

#[derive(Debug, Deserialize)]
struct Session {
    date: String,
    time: String,
}

impl Race {
    fn header(&self, offset: UtcOffset) -> String {
        format!(
            "🏎️ {}\n📍 {}, {}, {}\n🕒 Time: UTC{}",
            self.name,
            self.circuit.circuit_name,
            self.circuit.location.locality,
            self.circuit.location.country,
            format_offset(offset),
        )
    }

    fn session_lines(&self, view: ScheduleView, offset: UtcOffset) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        match view {
            ScheduleView::Weekend => {
                push_sessions(
                    &mut lines,
                    [
                        ("🔧 FP1", self.first_practice.as_ref()),
                        ("🔧 FP2", self.second_practice.as_ref()),
                        ("🔧 FP3", self.third_practice.as_ref()),
                        ("⚡ Sprint Qualifying", self.sprint_qualifying.as_ref()),
                        ("⚡ Sprint", self.sprint.as_ref()),
                        ("⏱️ Qualifying", self.qualifying.as_ref()),
                    ],
                    offset,
                )?;
                lines.push(self.format_race_session(offset)?);
            }
            ScheduleView::Qualifying => {
                push_sessions(
                    &mut lines,
                    [
                        ("⚡ Sprint Qualifying", self.sprint_qualifying.as_ref()),
                        ("⏱️ Qualifying", self.qualifying.as_ref()),
                    ],
                    offset,
                )?;
            }
            ScheduleView::Race => {
                push_sessions(&mut lines, [("⚡ Sprint", self.sprint.as_ref())], offset)?;
                lines.push(self.format_race_session(offset)?);
            }
        }

        Ok(lines)
    }

    fn format_race_session(&self, offset: UtcOffset) -> Result<String> {
        format_labeled_session("🏁 Race", &self.date, &self.time, offset)
    }
}

/// Fetch and format the next F1 race schedule.
///
/// # Errors
///
/// Returns an error if the API request fails, no race is available, or the API returns invalid
/// session date/time data.
pub async fn next_race_message(view: ScheduleView, offset: UtcOffset) -> Result<String> {
    let response = reqwest::get(NEXT_RACE_URL)
        .await
        .map_err(|e| Error::other(format!("failed to fetch F1 schedule: {e}")))?
        .error_for_status()
        .map_err(|e| Error::other(format!("failed to fetch F1 schedule: {e}")))?
        .json::<JolpicaResponse>()
        .await
        .map_err(|e| Error::other(format!("failed to parse F1 schedule: {e}")))?;

    let race = response
        .mr_data
        .race_table
        .races
        .first()
        .ok_or_else(|| Error::other("no upcoming F1 race found"))?;

    let lines = race.session_lines(view, offset)?;
    if lines.is_empty() {
        return Err(Error::other("no matching F1 sessions found"));
    }

    Ok(format!("{}\n\n{}", race.header(offset), lines.join("\n")))
}

fn push_sessions<const N: usize>(
    lines: &mut Vec<String>,
    sessions: [(&str, Option<&Session>); N],
    offset: UtcOffset,
) -> Result<()> {
    for (label, session) in sessions {
        let Some(session) = session else {
            continue;
        };
        lines.push(format_labeled_session(
            label,
            &session.date,
            &session.time,
            offset,
        )?);
    }

    Ok(())
}

fn format_labeled_session(
    label: &str,
    date: &str,
    time: &str,
    offset: UtcOffset,
) -> Result<String> {
    format_session(date, time, offset).map(|session| format!("{label}: {session}"))
}

fn format_session(date: &str, time: &str, offset: UtcOffset) -> Result<String> {
    let session_time = parse_session_time(date, time)?.to_offset(offset);
    session_time
        .format(DISPLAY_FORMAT)
        .map_err(|e| Error::other(format!("failed to format F1 session time: {e}")))
}

fn parse_session_time(date: &str, time: &str) -> Result<OffsetDateTime> {
    let raw = format!("{date}T{time}");
    OffsetDateTime::parse(&raw, &Rfc3339)
        .map_err(|e| Error::other(format!("failed to parse F1 session time `{raw}`: {e}")))
}

fn format_offset(offset: UtcOffset) -> String {
    let total_minutes = offset.whole_minutes();
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
    use time::UtcOffset;

    #[test]
    fn format_session_applies_positive_offset() {
        let offset = UtcOffset::from_hms(3, 0, 0).expect("valid offset");

        let formatted = format_session("2026-06-28", "13:00:00Z", offset).expect("format session");

        assert_eq!(formatted, "Sun, 28 Jun 16:00");
    }

    #[test]
    fn format_session_handles_date_rollover() {
        let offset = UtcOffset::from_hms(-3, 0, 0).expect("valid offset");

        let formatted = format_session("2026-06-28", "01:00:00Z", offset).expect("format session");

        assert_eq!(formatted, "Sat, 27 Jun 22:00");
    }

    #[test]
    fn format_offset_omits_zero_minutes() {
        let offset = UtcOffset::from_hms(3, 0, 0).expect("valid offset");

        assert_eq!(format_offset(offset), "+3");
    }
}
