use crate::error::{Error, Result};
use reqwest::Client;
use serde::Deserialize;

const DRIVER_STANDINGS_URL: &str =
    "https://api.jolpi.ca/ergast/f1/current/driverStandings.json?limit=40";
const CONSTRUCTOR_STANDINGS_URL: &str =
    "https://api.jolpi.ca/ergast/f1/current/constructorStandings.json?limit=40";

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

/// Fetch and format the current F1 driver and constructor standings.
///
/// # Errors
///
/// Returns an error if either API request fails or no standings are available.
pub(super) async fn standings_message(client: &Client) -> Result<String> {
    let (drivers, constructors) = tokio::try_join!(
        fetch_standings::<DriverStanding>(client, DRIVER_STANDINGS_URL),
        fetch_standings::<ConstructorStanding>(client, CONSTRUCTOR_STANDINGS_URL),
    )?;

    if drivers.standings.is_empty() || constructors.standings.is_empty() {
        return Err(Error::MissingF1Standings);
    }

    Ok(format!(
        "{}\n\n{}",
        format_driver_standings(&drivers),
        format_constructor_standings(&constructors.standings),
    ))
}

async fn fetch_standings<T>(client: &Client, url: &str) -> Result<StandingsList<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let response = client
        .get(url)
        .send()
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

    for standing in &list.standings {
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

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_some};

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
}
