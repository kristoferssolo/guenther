use crate::{
    download::{DownloadResult, platform::twitter::status_id},
    error::{Error, Result},
};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use tempfile::tempdir;
use tokio::fs;
use tracing::debug;
use url::Url;

pub(super) const USER_AGENT: &str = concat!("guenther/", env!("CARGO_PKG_VERSION"));

pub async fn download_tweet_images(client: &Client, url: &str) -> Result<DownloadResult> {
    let tweet_id = extract_tweet_id(url).ok_or(Error::InvalidTwitterUrl)?;
    let payload = fetch_tweet_result(client, &tweet_id).await?;
    let image_urls = extract_photo_urls(&payload);

    if image_urls.is_empty() {
        return Err(Error::MissingTwitterImages);
    }

    let tempdir = tempdir()?;
    let mut files = Vec::with_capacity(image_urls.len());

    for (index, image_url) in image_urls.iter().enumerate() {
        let response = client
            .get(image_url)
            .send()
            .await
            .map_err(Error::DownloadTwitterImage)?;
        debug!(index, status = %response.status(), "Received Twitter image response");
        let bytes = response
            .error_for_status()
            .map_err(Error::DownloadTwitterImage)?
            .bytes()
            .await
            .map_err(Error::DownloadTwitterImage)?;

        let path = tempdir
            .path()
            .join(format!("twitter-{index}.{}", image_extension(image_url)));
        fs::write(&path, &bytes).await?;
        files.push(path);
    }

    Ok(DownloadResult {
        tempdir,
        files,
        source_text: parse_post_text_from_value(&payload),
    })
}

pub async fn fetch_tweet_text(client: &Client, url: &str) -> Result<Option<String>> {
    let tweet_id = extract_tweet_id(url).ok_or(Error::InvalidTwitterUrl)?;
    let payload = fetch_tweet_result(client, &tweet_id).await?;
    Ok(parse_post_text_from_value(&payload))
}

async fn fetch_tweet_result(client: &Client, tweet_id: &str) -> Result<Value> {
    let token = syndication_token(tweet_id);
    let url = format!(
        "https://cdn.syndication.twimg.com/tweet-result?id={tweet_id}&token={token}&lang=en"
    );

    let response = client
        .get(url)
        .send()
        .await
        .map_err(Error::FetchTwitterSyndication)?;
    debug!(status = %response.status(), "Received Twitter syndication response");
    response
        .error_for_status()
        .map_err(Error::FetchTwitterSyndication)?
        .json::<Value>()
        .await
        .map_err(Error::ParseTwitterSyndication)
}

fn extract_tweet_id(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    status_id(&parsed).map(str::to_owned)
}

/// The reference implementation formats this value in base 36. The endpoint
/// only rejects a missing or empty token and does not validate its value, so
/// the base-10 digits work.
fn syndication_token(tweet_id: &str) -> String {
    let id = tweet_id.parse::<f64>().unwrap_or_default();
    ((id / 1e15) * std::f64::consts::PI)
        .to_string()
        .chars()
        .filter(|ch| *ch != '.' && *ch != '0')
        .collect()
}

fn extract_photo_urls(payload: &Value) -> Vec<String> {
    let photos = payload
        .get("photos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|photo| photo.get("url").and_then(Value::as_str).map(str::to_owned));

    let media_details = payload
        .get("mediaDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|media| media.get("type").and_then(Value::as_str) == Some("photo"))
        .filter_map(|media| {
            media
                .get("media_url_https")
                .or_else(|| media.get("media_url"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });

    let mut seen = HashSet::new();
    photos
        .chain(media_details)
        .filter(|url| seen.insert(url.clone()))
        .collect()
}

fn parse_post_text_from_value(json: &Value) -> Option<String> {
    let text = json
        .get("full_text")
        .and_then(Value::as_str)
        .or_else(|| json.get("text").and_then(Value::as_str))
        .or_else(|| json.get("description").and_then(Value::as_str))
        .or_else(|| json.get("title").and_then(Value::as_str))?
        .trim();

    (!text.is_empty()).then(|| text.to_owned())
}

fn image_extension(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_owned))
        })
        .and_then(|last| {
            last.rsplit_once('.')
                .map(|(_, ext)| ext.to_ascii_lowercase())
        })
        .filter(|ext| matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif"))
        .unwrap_or_else(|| "jpg".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_some_eq;
    use rstest::rstest;
    use serde_json::json;

    #[test]
    fn extracts_tweet_id_from_status_url() {
        assert_eq!(
            extract_tweet_id("https://x.com/i/status/2037463638215462967").as_deref(),
            Some("2037463638215462967")
        );
    }

    #[test]
    fn extracts_tweet_id_after_status_segment() {
        assert_some_eq!(
            extract_tweet_id("https://x.com/12345/status/678").as_deref(),
            "678"
        );
    }

    #[test]
    fn extracts_photo_urls_in_tweet_order_without_duplicates() {
        let payload = json!({
            "photos": [
                { "url": "https://pbs.twimg.com/media/b.jpg" },
                { "url": "https://pbs.twimg.com/media/a.jpg" }
            ],
            "mediaDetails": [
                { "type": "photo", "media_url_https": "https://pbs.twimg.com/media/b.jpg" }
            ]
        });

        assert_eq!(
            extract_photo_urls(&payload),
            vec![
                "https://pbs.twimg.com/media/b.jpg".to_owned(),
                "https://pbs.twimg.com/media/a.jpg".to_owned()
            ]
        );
    }

    #[test]
    fn extracts_photo_urls_from_photos() {
        let payload = json!({
            "photos": [
                {
                    "url": "https://pbs.twimg.com/media/one.jpg"
                },
                {
                    "url": "https://pbs.twimg.com/media/two.png"
                }
            ]
        });

        assert_eq!(
            extract_photo_urls(&payload),
            vec![
                "https://pbs.twimg.com/media/one.jpg".to_owned(),
                "https://pbs.twimg.com/media/two.png".to_owned()
            ]
        );
    }

    #[test]
    fn extracts_photo_urls_from_media_details() {
        let payload = json!({
            "mediaDetails": [
                {
                    "type": "photo",
                    "media_url_https": "https://pbs.twimg.com/media/one.jpg"
                },
                {
                    "type": "video",
                    "media_url_https": "https://pbs.twimg.com/media/two.jpg"
                }
            ]
        });

        assert_eq!(
            extract_photo_urls(&payload),
            vec!["https://pbs.twimg.com/media/one.jpg".to_owned()]
        );
    }

    #[rstest]
    #[case::full_text(json!({"full_text": "hello world", "description": "fallback"}), Some("hello world"))]
    #[case::text(json!({"text": "tweet body"}), Some("tweet body"))]
    #[case::description(json!({"description": "fallback"}), Some("fallback"))]
    #[case::whitespace_only(json!({"full_text": "   "}), None)]
    fn parses_post_text(#[case] payload: Value, #[case] expected: Option<&str>) {
        assert_eq!(parse_post_text_from_value(&payload).as_deref(), expected);
    }

    #[test]
    fn syndication_token_is_not_empty() {
        assert_ne!(syndication_token("2037463638215462967"), "");
    }
}
