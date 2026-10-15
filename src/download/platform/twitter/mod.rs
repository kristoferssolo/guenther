mod syndication;

use crate::{
    download::{DownloadResult, platform::cobalt::CobaltClient},
    error::{Error, Result},
};
use reqwest::Client;
use tracing::warn;
use url::Url;

/// Extract the numeric post ID that follows `status` in a Twitter/X URL path,
/// such as `/<user>/status/<id>` or `/i/status/<id>`.
pub fn status_id(url: &Url) -> Option<&str> {
    let mut segments = url.path_segments()?;
    while let Some(segment) = segments.next() {
        if segment == "status"
            && let Some(id) = segments.next()
            && !id.is_empty()
            && id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Some(id);
        }
    }
    None
}

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
        let (cobalt_result, tweet) = tokio::join!(
            cobalt.download(url),
            syndication::fetch_tweet(&self.http, url),
        );

        let cobalt_error = match cobalt_result {
            Ok(mut result) => {
                result.source_text = match tweet {
                    Ok(payload) => syndication::parse_post_text_from_value(&payload),
                    Err(err) => {
                        warn!(%err, "Could not fetch post text from Twitter syndication");
                        None
                    }
                };
                return Ok(result);
            }
            Err(err) => err,
        };

        let fallback = match tweet {
            Ok(payload) => syndication::download_tweet_images(&self.http, &payload).await,
            Err(err) => Err(err),
        };

        match fallback {
            Ok(result) => {
                warn!(%cobalt_error, "Cobalt could not fetch Twitter media; used image fallback");
                Ok(result)
            }
            Err(fallback_error) => {
                warn!(%fallback_error, "Twitter image fallback was not applicable");
                Err(cobalt_error)
            }
        }
    }
}
