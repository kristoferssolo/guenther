use thiserror::Error;

pub type Result<T> = std::result::Result<T, F1Error>;

#[derive(Debug, Error)]
pub enum F1Error {
    #[error("Failed to build the HTTP client: {0}")]
    BuildHttpClient(#[source] reqwest::Error),

    #[error("Failed to fetch the F1 schedule: {0}")]
    FetchSchedule(#[source] reqwest::Error),

    #[error("Failed to decode the F1 schedule: {0}")]
    DecodeSchedule(#[source] reqwest::Error),

    #[error("No upcoming F1 race was found")]
    MissingRace,

    #[error("No matching F1 sessions were found")]
    MissingSessions,

    #[error("Failed to fetch the F1 standings: {0}")]
    FetchStandings(#[source] reqwest::Error),

    #[error("Failed to decode the F1 standings: {0}")]
    DecodeStandings(#[source] reqwest::Error),

    #[error("No F1 standings were found")]
    MissingStandings,

    #[error("Failed to parse F1 session time `{raw}`")]
    ParseSessionTime {
        raw: String,
        #[source]
        source: chrono::ParseError,
    },
}
