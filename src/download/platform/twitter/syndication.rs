use crate::{
    download::{DownloadResult, platform::twitter::metadata::parse_post_text_from_value},
    error::{Error, Result},
};
use reqwest::Client;
use serde_json::Value;
use tempfile::tempdir;
use tokio::fs;
use tracing::debug;
use url::Url;

const USER_AGENT: &str = concat!("guenther/", env!("CARGO_PKG_VERSION"));

pub async fn download_tweet_images(url: &str) -> Result<DownloadResult> {
    let tweet_id = extract_tweet_id(url).ok_or(Error::InvalidTwitterUrl)?;
    let payload = fetch_tweet_result(&tweet_id).await?;
    let image_urls = extract_photo_urls(&payload);

    if image_urls.is_empty() {
        return Err(Error::MissingTwitterImages);
    }

    let client = http_client()?;
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

pub async fn fetch_tweet_text(url: &str) -> Result<Option<String>> {
    let tweet_id = extract_tweet_id(url).ok_or(Error::InvalidTwitterUrl)?;
    let payload = fetch_tweet_result(&tweet_id).await?;
    Ok(parse_post_text_from_value(&payload))
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(Error::BuildHttpClient)
}

async fn fetch_tweet_result(tweet_id: &str) -> Result<Value> {
    let token = syndication_token(tweet_id);
    let url = format!(
        "https://cdn.syndication.twimg.com/tweet-result?id={tweet_id}&token={token}&lang=en"
    );

    let response = http_client()?
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
    parsed
        .path_segments()?
        .find(|segment| segment.chars().all(|ch| ch.is_ascii_digit()))
        .map(str::to_owned)
}

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

    let mut urls = photos.chain(media_details).collect::<Vec<_>>();
    urls.sort();
    urls.dedup();
    urls
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
    use serde_json::json;

    #[test]
    fn extracts_tweet_id_from_status_url() {
        assert_eq!(
            extract_tweet_id("https://x.com/i/status/2037463638215462967").as_deref(),
            Some("2037463638215462967")
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

    #[test]
    fn syndication_token_is_not_empty() {
        assert!(!syndication_token("2037463638215462967").is_empty());
    }
}
