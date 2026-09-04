mod metadata;
mod syndication;

use crate::{
    download::{DownloadResult, platform::cobalt::CobaltClient},
    error::{Error, Result},
};
use reqwest::Client;
use tracing::warn;

#[derive(Debug, Clone)]
pub(super) struct TwitterClient {
    http: Client,
}

impl TwitterClient {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .user_agent(syndication::USER_AGENT)
                .build()
                .map_err(Error::BuildHttpClient)?,
        })
    }

    pub(super) async fn download(
        &self,
        cobalt: &CobaltClient,
        url: &str,
    ) -> Result<DownloadResult> {
        match cobalt.download(url).await {
            Ok(mut result) => {
                result.source_text = match syndication::fetch_tweet_text(&self.http, url).await {
                    Ok(text) => text,
                    Err(err) => {
                        warn!(%err, "Could not fetch post text from Twitter syndication");
                        None
                    }
                };
                Ok(result)
            }
            Err(cobalt_error) => match syndication::download_tweet_images(&self.http, url).await {
                Ok(result) => {
                    warn!(%cobalt_error, "Cobalt could not fetch Twitter media; used image fallback");
                    Ok(result)
                }
                Err(fallback_error) => {
                    warn!(%fallback_error, "Twitter image fallback was not applicable");
                    Err(cobalt_error)
                }
            },
        }
    }
}
