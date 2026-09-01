use crate::error::{Error, Result};
use chrono::{DateTime, FixedOffset, TimeDelta, Utc};
use serde::Deserialize;

const NEXT_RACE_URL: &str = "https://api.jolpi.ca/ergast/f1/current/next.json";
const DRIVER_STANDINGS_URL: &str =
    "https://api.jolpi.ca/ergast/f1/current/driverStandings.json?limit=40";
const CONSTRUCTOR_STANDINGS_URL: &str =
    "https://api.jolpi.ca/ergast/f1/current/constructorStandings.json?limit=40";
const DISPLAY_FORMAT: &str = "%a, %d %b %H:%M";

/// Number of driver standings lines to show; the API returns the full field.
const DRIVER_STANDINGS_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleView {
    Weekend,
    Qualifying,
    Race,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct JolpicaResponse {
    #[serde(rename = "MRData")]
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
struct Race {
    #[serde(rename = "raceName")]
    name: String,
    #[serde(rename = "Circuit")]
    circuit: Circuit,
    date: String,
    time: String,
    #[serde(rename = "FirstPractice")]
    first_practice: Option<Session>,
    #[serde(rename = "SecondPractice")]
    second_practice: Option<Session>,
    #[serde(rename = "ThirdPractice")]
    third_practice: Option<Session>,
    #[serde(rename = "Sprint")]
    sprint: Option<Session>,
    #[serde(rename = "SprintQualifying")]
    sprint_qualifying: Option<Session>,
    #[serde(rename = "Qualifying")]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StandingsResponse<T> {
    #[serde(rename = "MRData")]
    mr_data: StandingsMrData<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StandingsMrData<T> {
    standings_table: StandingsTable<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StandingsTable<T> {
    standings_lists: Vec<StandingsList<T>>,
}

#[derive(Debug, Deserialize)]
struct StandingsList<T> {
    season: String,
    round: String,
    #[serde(alias = "DriverStandings", rename = "ConstructorStandings")]
    standings: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriverStanding {
    position: String,
    points: String,
    #[serde(rename = "Driver")]
    driver: Driver,
    #[serde(rename = "Constructors")]
    constructors: Vec<Constructor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Driver {
    family_name: String,
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConstructorStanding {
    position: String,
    points: String,
    #[serde(rename = "Constructor")]
    constructor: Constructor,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constructor {
    name: String,
}

impl Race {
    /// All sessions of this race weekend as `(label, start)` pairs in weekend order, race last.
    fn sessions(&self) -> Result<Vec<(&'static str, DateTime<Utc>)>> {
        let mut sessions = Vec::new();
        for (label, session) in [
            ("🔧 FP1", self.first_practice.as_ref()),
            ("🔧 FP2", self.second_practice.as_ref()),
            ("🔧 FP3", self.third_practice.as_ref()),
            ("⚡ Sprint Qualifying", self.sprint_qualifying.as_ref()),
            ("⚡ Sprint", self.sprint.as_ref()),
            ("⏱️ Qualifying", self.qualifying.as_ref()),
        ] {
            let Some(session) = session else {
                continue;
            };
            let start = parse_session_time(&session.date, &session.time)?.with_timezone(&Utc);
            sessions.push((label, start));
        }

        let race_start = parse_session_time(&self.date, &self.time)?.with_timezone(&Utc);
        sessions.push(("🏁 Race", race_start));

        Ok(sessions)
    }

    fn header(&self, offset: FixedOffset) -> String {
        format!(
            "🏎️ {}\n📍 {}, {}, {}\n🕒 Time: UTC{}",
            self.name,
            self.circuit.circuit_name,
            self.circuit.location.locality,
            self.circuit.location.country,
            format_offset(offset),
        )
    }

    fn session_lines(&self, view: ScheduleView, offset: FixedOffset) -> Result<Vec<String>> {
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

    fn format_race_session(&self, offset: FixedOffset) -> Result<String> {
        format_labeled_session("🏁 Race", &self.date, &self.time, offset)
    }
}

/// Fetch and format the next F1 race schedule.
///
/// # Errors
///
/// Returns an error if the API request fails, no race is available, or the API returns invalid
/// session date/time data.
pub async fn next_race_message(view: ScheduleView, offset: FixedOffset) -> Result<String> {
    let race = next_race().await?;

    let lines = race.session_lines(view, offset)?;
    if lines.is_empty() {
        return Err(Error::MissingF1Sessions);
    }

    Ok(format!("{}\n\n{}", race.header(offset), lines.join("\n")))
}

/// Fetch the next F1 race weekend from the schedule API.
async fn next_race() -> Result<Race> {
    let response = reqwest::get(NEXT_RACE_URL)
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

/// Fetch and format a countdown to the next F1 session of the upcoming race weekend.
///
/// # Errors
///
/// Returns an error if the API request fails, no race is available, the API returns invalid
/// session date/time data, or every session of the weekend has already started.
pub async fn countdown_message(offset: FixedOffset) -> Result<String> {
    let race = next_race().await?;
    let now = Utc::now();

    let sessions = race.sessions()?;
    let Some((label, start)) = next_session(&sessions, now) else {
        return Err(Error::MissingF1Sessions);
    };

    let duration = start.signed_duration_since(now);
    let starts = start.with_timezone(&offset).format(DISPLAY_FORMAT);

    Ok(format!(
        "{}\n\n{}\n🕒 Starts: {starts}",
        race.header(offset),
        next_session_line(label, duration),
    ))
}

/// Fetch and format the current F1 driver and constructor standings.
///
/// # Errors
///
/// Returns an error if either API request fails or no standings are available.
pub async fn standings_message() -> Result<String> {
    let drivers = fetch_standings::<DriverStanding>(DRIVER_STANDINGS_URL).await?;
    let constructors = fetch_standings::<ConstructorStanding>(CONSTRUCTOR_STANDINGS_URL).await?;

    if drivers.standings.is_empty() || constructors.standings.is_empty() {
        return Err(Error::MissingF1Standings);
    }

    Ok(format!(
        "{}\n\n{}",
        format_driver_standings(&drivers),
        format_constructor_standings(&constructors.standings),
    ))
}

async fn fetch_standings<T>(url: &str) -> Result<StandingsList<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let response = reqwest::get(url)
        .await
        .map_err(Error::FetchF1Standings)?
        .error_for_status()
        .map_err(Error::FetchF1Standings)?
        .json::<StandingsResponse<T>>()
        .await
        .map_err(Error::DecodeF1Standings)?;

    response
        .mr_data
        .standings_table
        .standings_lists
        .into_iter()
        .next()
        .ok_or(Error::MissingF1Standings)
}

fn format_driver_standings(list: &StandingsList<DriverStanding>) -> String {
    let mut lines = vec![format!(
        "🏆 F1 Standings – {}, round {}",
        list.season, list.round,
    )];

    for standing in list.standings.iter().take(DRIVER_STANDINGS_LIMIT) {
        lines.push(format_driver_line(standing));
    }

    lines.join("\n")
}

fn format_driver_line(standing: &DriverStanding) -> String {
    let driver = &standing.driver;
    let name = driver.code.as_deref().unwrap_or(&driver.family_name);
    let team = standing
        .constructors
        .first()
        .map_or_else(String::new, |constructor| {
            format!(" ({})", constructor.name)
        });

    format!(
        "{}{}. {name}{team} – {} pts",
        position_medal(&standing.position),
        standing.position,
        standing.points,
    )
}

fn format_constructor_standings(standings: &[ConstructorStanding]) -> String {
    let mut lines = vec!["🏗️ Constructors".to_string()];

    for standing in standings {
        lines.push(format!(
            "{}{}. {} – {} pts",
            position_medal(&standing.position),
            standing.position,
            standing.constructor.name,
            standing.points,
        ));
    }

    lines.join("\n")
}

fn position_medal(position: &str) -> &'static str {
    match position {
        "1" => "🥇 ",
        "2" => "🥈 ",
        "3" => "🥉 ",
        _ => "",
    }
}

fn push_sessions<const N: usize>(
    lines: &mut Vec<String>,
    sessions: [(&str, Option<&Session>); N],
    offset: FixedOffset,
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
    offset: FixedOffset,
) -> Result<String> {
    format_session(date, time, offset).map(|session| format!("{label}: {session}"))
}

fn format_session(date: &str, time: &str, offset: FixedOffset) -> Result<String> {
    let session_time = parse_session_time(date, time)?.with_timezone(&offset);
    Ok(session_time.format(DISPLAY_FORMAT).to_string())
}

fn parse_session_time(date: &str, time: &str) -> Result<DateTime<FixedOffset>> {
    let raw = format!("{date}T{time}");
    DateTime::parse_from_rfc3339(&raw).map_err(|source| Error::ParseF1SessionTime { raw, source })
}

/// First session of `sessions` that has not started yet; a session counts as past
/// once its start time is reached.
fn next_session(
    sessions: &[(&'static str, DateTime<Utc>)],
    now: DateTime<Utc>,
) -> Option<(&'static str, DateTime<Utc>)> {
    sessions.iter().find(|(_, start)| *start > now).copied()
}

/// Human-readable countdown line, reusing the session's emoji label.
fn next_session_line(label: &str, duration: TimeDelta) -> String {
    let (emoji, session) = label.split_once(' ').unwrap_or(("⏱️", label));
    format!(
        "{emoji} Next session: {session} – in {}",
        format_duration(duration)
    )
}

/// Format a duration as `"2d 4h 30m"`, omitting zero units; anything under a
/// minute renders as `"under a minute"`.
fn format_duration(duration: TimeDelta) -> String {
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

fn format_offset(offset: FixedOffset) -> String {
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
    use claims::{assert_none, assert_ok, assert_some};

    #[test]
    fn decodes_jolpica_next_race_response() {
        let response = assert_ok!(serde_json::from_str::<JolpicaResponse>(
            r#"{
                "MRData": {
                    "RaceTable": {
                        "Races": [{
                            "raceName": "Austrian Grand Prix",
                            "Circuit": {
                                "circuitName": "Red Bull Ring",
                                "Location": {
                                    "locality": "Spielberg",
                                    "country": "Austria"
                                }
                            },
                            "date": "2026-06-28",
                            "time": "13:00:00Z",
                            "FirstPractice": {
                                "date": "2026-06-26",
                                "time": "11:30:00Z"
                            },
                            "Qualifying": {
                                "date": "2026-06-27",
                                "time": "14:00:00Z"
                            }
                        }]
                    }
                }
            }"#,
        ));

        let race = assert_some!(response.mr_data.race_table.races.first());

        assert_eq!(race.name, "Austrian Grand Prix");
        assert_eq!(race.circuit.circuit_name, "Red Bull Ring");
        assert!(race.first_practice.is_some());
        assert!(race.qualifying.is_some());
    }

    #[test]
    fn format_session_applies_positive_offset() {
        let offset = assert_some!(FixedOffset::east_opt(3 * 3_600));

        let formatted = assert_ok!(format_session("2026-06-28", "13:00:00Z", offset));

        assert_eq!(formatted, "Sun, 28 Jun 16:00");
    }

    #[test]
    fn format_session_handles_date_rollover() {
        let offset = assert_some!(FixedOffset::west_opt(3 * 3_600));

        let formatted = assert_ok!(format_session("2026-06-28", "01:00:00Z", offset));

        assert_eq!(formatted, "Sat, 27 Jun 22:00");
    }

    #[test]
    fn format_offset_omits_zero_minutes() {
        let offset = assert_some!(FixedOffset::east_opt(3 * 3_600));

        assert_eq!(format_offset(offset), "+3");
    }

    #[test]
    fn decodes_jolpica_driver_standings_response() {
        let list = assert_ok!(serde_json::from_str::<StandingsList<DriverStanding>>(
            r#"{
                "season": "2026",
                "round": "12",
                "DriverStandings": [
                    {
                        "position": "1",
                        "points": "242",
                        "wins": "8",
                        "Driver": {
                            "driverId": "antonelli",
                            "givenName": "Kimi",
                            "familyName": "Antonelli",
                            "code": "ANT"
                        },
                        "Constructors": [{"constructorId": "mercedes", "name": "Mercedes"}]
                    },
                    {
                        "position": "2",
                        "points": "183",
                        "wins": "3",
                        "Driver": {
                            "driverId": "russell",
                            "givenName": "George",
                            "familyName": "Russell"
                        },
                        "Constructors": [{"constructorId": "mercedes", "name": "Mercedes"}]
                    }
                ]
            }"#,
        ));

        assert_eq!(list.season, "2026");
        assert_eq!(list.round, "12");

        let first = assert_some!(list.standings.first());
        assert_eq!(first.position, "1");
        assert_eq!(first.points, "242");
        assert_eq!(first.driver.code.as_deref(), Some("ANT"));
        assert_eq!(
            first.constructors.first().map(|c| c.name.as_str()),
            Some("Mercedes")
        );

        let second = assert_some!(list.standings.get(1));
        assert_none!(second.driver.code.as_ref());
    }

    #[test]
    fn decodes_jolpica_constructor_standings_response() {
        let list = assert_ok!(serde_json::from_str::<StandingsList<ConstructorStanding>>(
            r#"{
                "season": "2026",
                "round": "12",
                "ConstructorStandings": [
                    {
                        "position": "1",
                        "points": "425",
                        "wins": "11",
                        "Constructor": {"constructorId": "mercedes", "name": "Mercedes"}
                    }
                ]
            }"#,
        ));

        assert_eq!(list.season, "2026");
        assert_eq!(list.round, "12");

        let first = assert_some!(list.standings.first());
        assert_eq!(first.position, "1");
        assert_eq!(first.points, "425");
        assert_eq!(first.constructor.name, "Mercedes");
    }

    #[test]
    fn format_driver_standings_uses_medals_codes_and_header() {
        let driver = |position: &str, points: &str, code: Option<&str>| DriverStanding {
            position: position.to_string(),
            points: points.to_string(),
            driver: Driver {
                family_name: "Antonelli".to_string(),
                code: code.map(str::to_string),
            },
            constructors: vec![Constructor {
                name: "Mercedes".to_string(),
            }],
        };
        let list = StandingsList {
            season: "2026".to_string(),
            round: "12".to_string(),
            standings: vec![
                driver("1", "242", Some("ANT")),
                driver("2", "183", Some("RUS")),
                driver("3", "183", None),
                driver("4", "170", None),
            ],
        };

        let formatted = format_driver_standings(&list);

        assert_eq!(
            formatted,
            "🏆 F1 Standings – 2026, round 12\n\
             🥇 1. ANT (Mercedes) – 242 pts\n\
             🥈 2. RUS (Mercedes) – 183 pts\n\
             🥉 3. Antonelli (Mercedes) – 183 pts\n\
             4. Antonelli (Mercedes) – 170 pts"
        );
    }

    #[test]
    fn format_constructor_standings_uses_medals() {
        let standings = vec![
            ConstructorStanding {
                position: "1".to_string(),
                points: "425".to_string(),
                constructor: Constructor {
                    name: "Mercedes".to_string(),
                },
            },
            ConstructorStanding {
                position: "2".to_string(),
                points: "338".to_string(),
                constructor: Constructor {
                    name: "Ferrari".to_string(),
                },
            },
        ];

        let formatted = format_constructor_standings(&standings);

        assert_eq!(
            formatted,
            "🏗️ Constructors\n\
             🥇 1. Mercedes – 425 pts\n\
             🥈 2. Ferrari – 338 pts"
        );
    }

    fn countdown_race() -> Race {
        let response = assert_ok!(serde_json::from_str::<JolpicaResponse>(
            r#"{
                "MRData": {
                    "RaceTable": {
                        "Races": [{
                            "raceName": "Austrian Grand Prix",
                            "Circuit": {
                                "circuitName": "Red Bull Ring",
                                "Location": {
                                    "locality": "Spielberg",
                                    "country": "Austria"
                                }
                            },
                            "date": "2026-06-28",
                            "time": "13:00:00Z",
                            "FirstPractice": {
                                "date": "2026-06-26",
                                "time": "11:30:00Z"
                            },
                            "SecondPractice": {
                                "date": "2026-06-26",
                                "time": "15:00:00Z"
                            },
                            "ThirdPractice": {
                                "date": "2026-06-27",
                                "time": "10:30:00Z"
                            },
                            "SprintQualifying": {
                                "date": "2026-06-27",
                                "time": "14:00:00Z"
                            },
                            "Sprint": {
                                "date": "2026-06-28",
                                "time": "09:00:00Z"
                            },
                            "Qualifying": {
                                "date": "2026-06-27",
                                "time": "16:00:00Z"
                            }
                        }]
                    }
                }
            }"#,
        ));

        assert_some!(response.mr_data.race_table.races.into_iter().next())
    }

    #[test]
    fn next_session_picks_fp1_before_the_weekend() {
        let race = countdown_race();
        let sessions = assert_ok!(race.sessions());
        let now =
            assert_ok!(DateTime::parse_from_rfc3339("2026-06-25T08:00:00Z")).with_timezone(&Utc);

        let next = assert_some!(next_session(&sessions, now));

        assert_eq!(next.0, "🔧 FP1");
    }

    #[test]
    fn next_session_skips_started_sessions() {
        let race = countdown_race();
        let sessions = assert_ok!(race.sessions());
        let now =
            assert_ok!(DateTime::parse_from_rfc3339("2026-06-27T11:00:00Z")).with_timezone(&Utc);

        let next = assert_some!(next_session(&sessions, now));

        assert_eq!(next.0, "⚡ Sprint Qualifying");
    }

    #[test]
    fn next_session_picks_race_when_everything_else_started() {
        let race = countdown_race();
        let sessions = assert_ok!(race.sessions());
        let now =
            assert_ok!(DateTime::parse_from_rfc3339("2026-06-28T10:00:00Z")).with_timezone(&Utc);

        let next = assert_some!(next_session(&sessions, now));

        assert_eq!(next.0, "🏁 Race");
    }

    #[test]
    fn next_session_is_none_when_all_sessions_started() {
        let race = countdown_race();
        let sessions = assert_ok!(race.sessions());
        let now =
            assert_ok!(DateTime::parse_from_rfc3339("2026-06-28T13:00:00Z")).with_timezone(&Utc);

        assert_none!(next_session(&sessions, now));
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
