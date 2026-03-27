use crate::{
    download::DownloadResult,
    error::{Error, Result},
};
use reqwest::Client;
use serde_json::Value;
use tempfile::tempdir;
use tokio::fs;
use url::Url;

use super::metadata::parse_post_text_from_value;

pub async fn download_tweet_images(url: &str) -> Result<DownloadResult> {
    let tweet_id =
        extract_tweet_id(url).ok_or_else(|| Error::other("failed to extract tweet id"))?;
    let payload = fetch_tweet_result(&tweet_id).await?;
    let image_urls = extract_photo_urls(&payload);

    if image_urls.is_empty() {
        return Err(Error::ytdlp_failed(
            "ERROR: [twitter] no downloadable images found in this tweet",
        ));
    }

    let client = http_client()?;
    let tempdir = tempdir()?;
    let mut files = Vec::with_capacity(image_urls.len());

    for (index, image_url) in image_urls.iter().enumerate() {
        let bytes = client
            .get(image_url)
            .send()
            .await
            .map_err(download_error)?
            .error_for_status()
            .map_err(download_error)?
            .bytes()
            .await
            .map_err(|e| Error::other(format!("failed to read twitter image bytes: {e}")))?;

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

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent("guenther/0.1.0")
        .build()
        .map_err(|e| Error::other(format!("failed to build reqwest client: {e}")))
}

async fn fetch_tweet_result(tweet_id: &str) -> Result<Value> {
    let token = syndication_token(tweet_id);
    let url = format!(
        "https://cdn.syndication.twimg.com/tweet-result?id={tweet_id}&token={token}&lang=en"
    );

    http_client()?
        .get(url)
        .send()
        .await
        .map_err(fetch_error)?
        .error_for_status()
        .map_err(fetch_error)?
        .json::<Value>()
        .await
        .map_err(|e| Error::other(format!("failed to parse twitter syndication data: {e}")))
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
                .and_then(|segments| segments.last().map(str::to_owned))
        })
        .and_then(|last| {
            last.rsplit_once('.')
                .map(|(_, ext)| ext.to_ascii_lowercase())
        })
        .filter(|ext| matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif"))
        .unwrap_or_else(|| "jpg".to_owned())
}

fn fetch_error(err: reqwest::Error) -> Error {
    Error::other(format!("failed to fetch twitter syndication data: {err}"))
}

fn download_error(err: reqwest::Error) -> Error {
    Error::other(format!("failed to download twitter image: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{extract_photo_urls, extract_tweet_id, syndication_token};
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
                {"url": "https://pbs.twimg.com/media/one.jpg"},
                {"url": "https://pbs.twimg.com/media/two.png"}
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
                {"type": "photo", "media_url_https": "https://pbs.twimg.com/media/one.jpg"},
                {"type": "video", "media_url_https": "https://pbs.twimg.com/media/two.jpg"}
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
