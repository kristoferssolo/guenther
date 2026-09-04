mod cobalt;
mod twitter;

use crate::{
    config::{CobaltConfig, Platform},
    download::DownloadResult,
    error::Result,
};
use cobalt::CobaltClient;
use twitter::TwitterClient;

/// Downloads media for every supported platform using reusable HTTP clients.
#[derive(Debug, Clone)]
pub struct Downloader {
    cobalt: CobaltClient,
    twitter: TwitterClient,
}

impl Downloader {
    /// Build the platform clients from the application configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if an HTTP client cannot be built.
    pub fn new(cobalt: CobaltConfig) -> Result<Self> {
        Ok(Self {
            cobalt: CobaltClient::new(cobalt)?,
            twitter: TwitterClient::new()?,
        })
    }

    /// Download media from a supported platform URL.
    ///
    /// # Errors
    ///
    /// Returns errors from the selected platform downloader.
    pub async fn download(&self, platform: Platform, url: &str) -> Result<DownloadResult> {
        match platform {
            Platform::Twitter => self.twitter.download(&self.cobalt, url).await,
            Platform::Instagram | Platform::Tiktok | Platform::Youtube => {
                self.cobalt.download(url).await
            }
        }
    }
}
