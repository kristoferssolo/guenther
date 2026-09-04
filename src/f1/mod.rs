mod schedule;
mod standings;

use crate::{
    config::F1Config,
    error::{Error, Result},
};
use reqwest::Client;

pub use schedule::ScheduleView;

/// Fetches and formats Formula 1 data using one reusable HTTP client.
#[derive(Debug, Clone)]
pub struct F1 {
    http: Client,
    config: F1Config,
}

impl F1 {
    /// Build the Formula 1 client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(config: F1Config) -> Result<Self> {
        let http = Client::builder().build().map_err(Error::BuildHttpClient)?;
        Ok(Self { http, config })
    }

    /// Fetch and format the requested sessions from the next race weekend.
    ///
    /// # Errors
    ///
    /// Returns an error for failed requests, missing races, or invalid session data.
    pub async fn schedule(&self, view: ScheduleView) -> Result<String> {
        schedule::next_race_message(&self.http, view, self.config.utc_offset).await
    }

    /// Fetch and format the countdown to the next race-weekend session.
    ///
    /// # Errors
    ///
    /// Returns an error for failed requests, missing races, or invalid session data.
    pub async fn countdown(&self) -> Result<String> {
        schedule::countdown_message(&self.http, self.config.utc_offset).await
    }

    /// Fetch and format the current driver and constructor standings.
    ///
    /// # Errors
    ///
    /// Returns an error if either request fails or either standings list is empty.
    pub async fn standings(&self) -> Result<String> {
        standings::standings_message(&self.http).await
    }
}
