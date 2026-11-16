//! Reads web pages that chat members link when they talk to Guenther.
//!
//! Links come from chat users, so the client only connects to public
//! addresses, including after redirects.

mod decode;
mod extract;
mod guard;

pub use crate::web_page::extract::WebPage;

use crate::web_page::{
    decode::decode_html,
    guard::{PublicResolver, is_allowed_url},
};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
    redirect::{Attempt, Policy},
};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use url::Url;

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;
/// The metadata and opening paragraphs fit well within this; the rest of a
/// larger page is not read.
const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; guenther/",
    env!("CARGO_PKG_VERSION"),
    ")"
);

#[derive(Debug, Error)]
pub enum WebPageError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Only public http(s) URLs are fetched: {0}")]
    BlockedUrl(Url),

    #[error("Server responded with {0}")]
    Status(StatusCode),

    #[error("Not an HTML page: {0}")]
    NotHtml(String),
}

pub type Result<T> = std::result::Result<T, WebPageError>;

/// HTTP client for linked pages.
#[derive(Debug, Clone)]
pub struct PageFetcher {
    http: Client,
}

impl PageFetcher {
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .timeout(FETCH_TIMEOUT)
            .user_agent(USER_AGENT)
            // A proxy would resolve hosts itself, bypassing the resolver.
            .no_proxy()
            .dns_resolver(Arc::new(PublicResolver))
            .redirect(Policy::custom(redirect))
            .build()?;
        Ok(Self { http })
    }

    /// Fetch `url` and extract what the page says about itself.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL is not a public http(s) URL, the request
    /// fails, or the response is not an HTML page.
    pub async fn fetch(&self, url: &Url) -> Result<WebPage> {
        if !is_allowed_url(url) {
            return Err(WebPageError::BlockedUrl(url.clone()));
        }
        let mut response = self
            .http
            .get(url.clone())
            .header(ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(WebPageError::Status(status));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if !content_type.contains("html") {
            return Err(WebPageError::NotHtml(content_type));
        }

        let mut html = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            let room = MAX_HTML_BYTES.saturating_sub(html.len());
            html.extend_from_slice(chunk.get(..room).unwrap_or(&chunk));
            if html.len() >= MAX_HTML_BYTES {
                break;
            }
        }
        Ok(WebPage::parse(&decode_html(&html, &content_type)))
    }
}

fn redirect(attempt: Attempt) -> reqwest::redirect::Action {
    if attempt.previous().len() >= MAX_REDIRECTS {
        return attempt.error("too many redirects");
    }
    if !is_allowed_url(attempt.url()) {
        let url = attempt.url().clone();
        return attempt.error(WebPageError::BlockedUrl(url));
    }
    attempt.follow()
}
