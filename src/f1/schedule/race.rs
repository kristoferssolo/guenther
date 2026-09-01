use super::format::{format_offset, parse_session_time};
use crate::error::Result;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;

/// A session of a race weekend, in the order it appears on the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionKind {
    FirstPractice,
    SecondPractice,
    ThirdPractice,
    SprintQualifying,
    Sprint,
    Qualifying,
    Race,
}

impl SessionKind {
    /// Emoji shown before the session name wherever it is rendered.
    pub(super) const fn emoji(self) -> &'static str {
        match self {
            Self::FirstPractice | Self::SecondPractice | Self::ThirdPractice => "🔧",
            Self::SprintQualifying | Self::Sprint => "⚡",
            Self::Qualifying => "⏱️",
            Self::Race => "🏁",
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::FirstPractice => "FP1",
            Self::SecondPractice => "FP2",
            Self::ThirdPractice => "FP3",
            Self::SprintQualifying => "Sprint Qualifying",
            Self::Sprint => "Sprint",
            Self::Qualifying => "Qualifying",
            Self::Race => "Race",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct JolpicaResponse {
    #[serde(rename = "MRData")]
    pub(super) mr_data: MrData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct MrData {
    pub(super) race_table: RaceTable,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RaceTable {
    pub(super) races: Vec<Race>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Race {
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

impl Race {
    /// Every session of this race weekend in schedule order, race last.
    ///
    /// Sessions the API omits — practices on a sprint weekend, for instance — are skipped.
    pub(super) fn sessions(&self) -> Result<Vec<(SessionKind, DateTime<Utc>)>> {
        let optional = [
            (SessionKind::FirstPractice, self.first_practice.as_ref()),
            (SessionKind::SecondPractice, self.second_practice.as_ref()),
            (SessionKind::ThirdPractice, self.third_practice.as_ref()),
            (
                SessionKind::SprintQualifying,
                self.sprint_qualifying.as_ref(),
            ),
            (SessionKind::Sprint, self.sprint.as_ref()),
            (SessionKind::Qualifying, self.qualifying.as_ref()),
        ];

        let mut sessions = optional
            .into_iter()
            .filter_map(|(kind, session)| session.map(|session| (kind, session)))
            .map(|(kind, session)| {
                parse_session_time(&session.date, &session.time).map(|start| (kind, start))
            })
            .collect::<Result<Vec<_>>>()?;

        sessions.push((
            SessionKind::Race,
            parse_session_time(&self.date, &self.time)?,
        ));

        Ok(sessions)
    }

    pub(super) fn header(&self, offset: FixedOffset) -> String {
        format!(
            "🏎️ {}\n📍 {}, {}, {}\n🕒 Time: UTC{}",
            self.name,
            self.circuit.circuit_name,
            self.circuit.location.locality,
            self.circuit.location.country,
            format_offset(offset),
        )
    }
}

/// First session whose start time is after `now`.
pub(super) fn next_session(
    sessions: &[(SessionKind, DateTime<Utc>)],
    now: DateTime<Utc>,
) -> Option<(SessionKind, DateTime<Utc>)> {
    sessions.iter().find(|(_, start)| *start > now).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_some};

    fn parse_races(json: &str) -> Vec<Race> {
        let response = assert_ok!(serde_json::from_str::<JolpicaResponse>(json));
        response.mr_data.race_table.races
    }

    /// A weekend with a partial session list, as the API reports it before practice times land.
    fn partial_race() -> Race {
        let races = parse_races(
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
        );

        assert_some!(races.into_iter().next())
    }

    /// A full sprint weekend with every session populated.
    fn sprint_race() -> Race {
        let races = parse_races(
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
        );

        assert_some!(races.into_iter().next())
    }

    fn at(timestamp: &str) -> DateTime<Utc> {
        assert_ok!(DateTime::parse_from_rfc3339(timestamp)).with_timezone(&Utc)
    }

    #[test]
    fn decodes_jolpica_next_race_response() {
        let race = partial_race();

        assert_eq!(race.name, "Austrian Grand Prix");
        assert_eq!(race.circuit.circuit_name, "Red Bull Ring");
        assert_eq!(race.circuit.location.locality, "Spielberg");
    }

    #[test]
    fn sessions_skip_the_ones_the_api_omits() {
        let sessions = assert_ok!(partial_race().sessions());

        let kinds: Vec<_> = sessions.into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(
            kinds,
            [
                SessionKind::FirstPractice,
                SessionKind::Qualifying,
                SessionKind::Race
            ]
        );
    }

    #[test]
    fn next_session_picks_fp1_before_the_weekend() {
        let sessions = assert_ok!(sprint_race().sessions());

        let next = assert_some!(next_session(&sessions, at("2026-06-25T08:00:00Z")));

        assert_eq!(next.0, SessionKind::FirstPractice);
    }

    #[test]
    fn next_session_skips_started_sessions() {
        let sessions = assert_ok!(sprint_race().sessions());

        let next = assert_some!(next_session(&sessions, at("2026-06-27T11:00:00Z")));

        assert_eq!(next.0, SessionKind::SprintQualifying);
    }

    #[test]
    fn next_session_picks_race_when_everything_else_started() {
        let sessions = assert_ok!(sprint_race().sessions());

        let next = assert_some!(next_session(&sessions, at("2026-06-28T10:00:00Z")));

        assert_eq!(next.0, SessionKind::Race);
    }

    #[test]
    fn next_session_is_none_when_all_sessions_started() {
        let sessions = assert_ok!(sprint_race().sessions());

        assert_none!(next_session(&sessions, at("2026-06-28T13:00:00Z")));
    }
}
