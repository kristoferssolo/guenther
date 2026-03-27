use crate::{
    config::global_config,
    download::{DownloadResult, platform::run_yt_dlp},
    error::{Error, Result},
};
use reqwest::Client;
use serde_json::Value;
use std::path::Path;
use tempfile::tempdir;
use tokio::fs;
use tracing::warn;
use url::Url;

/// Download a Twitter URL with yt-dlp.
///
/// # Errors
///
/// - Propagates `run_command_in_tempdir` errors.
pub async fn download_twitter(url: String) -> Result<DownloadResult> {
    let config = global_config();
    match run_yt_dlp(
        &["-t", "mp4", "--write-info-json"],
        config.twitter.cookies_path.as_ref(),
        &url,
    )
    .await
    {
        Ok(mut result) => {
            result.source_text = extract_twitter_post_text(result.tempdir.path()).await;
            Ok(result)
        }
        Err(err) if is_no_video_tweet_error(&err) => {
            warn!(url = %url, %err, "yt-dlp could not fetch twitter media; falling back to image downloader");
            download_twitter_images_via_syndication(&url).await
        }
        Err(err) => Err(err),
    }
}

async fn extract_twitter_post_text(root: &Path) -> Option<String> {
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut read_dir = fs::read_dir(&dir).await.ok()?;
        while let Some(entry) = read_dir.next_entry().await.ok()? {
            let path = entry.path();
            let file_type = entry.file_type().await.ok()?;

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }

            if !is_info_json(&path) {
                continue;
            }

            let content = fs::read_to_string(&path).await.ok()?;
            if let Some(text) = parse_twitter_post_text(&content) {
                return Some(text);
            }
        }
    }

    None
}

fn is_info_json(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".info.json"))
}

async fn download_twitter_images_via_syndication(url: &str) -> Result<DownloadResult> {
    let tweet_id =
        extract_tweet_id(url).ok_or_else(|| Error::other("failed to extract tweet id"))?;
    let payload = fetch_tweet_result(&tweet_id).await?;
    let image_urls = extract_photo_urls(&payload);

    if image_urls.is_empty() {
        return Err(Error::ytdlp_failed(
            "ERROR: [twitter] no downloadable images found in this tweet",
        ));
    }

    let client = Client::builder()
        .user_agent("guenther/0.1.0")
        .build()
        .map_err(|e| Error::other(format!("failed to build reqwest client: {e}")))?;
    let tempdir = tempdir()?;
    let mut files = Vec::with_capacity(image_urls.len());

    for (index, image_url) in image_urls.iter().enumerate() {
        let bytes = client
            .get(image_url)
            .send()
            .await
            .map_err(|e| Error::other(format!("failed to download twitter image: {e}")))?
            .error_for_status()
            .map_err(|e| Error::other(format!("failed to download twitter image: {e}")))?
            .bytes()
            .await
            .map_err(|e| Error::other(format!("failed to read twitter image bytes: {e}")))?;

        let extension = image_extension(image_url);
        let path = tempdir.path().join(format!("twitter-{index}.{extension}"));
        fs::write(&path, &bytes).await?;
        files.push(path);
    }

    Ok(DownloadResult {
        tempdir,
        files,
        source_text: parse_twitter_post_text_from_value(&payload),
    })
}

async fn fetch_tweet_result(tweet_id: &str) -> Result<Value> {
    let token = syndication_token(tweet_id);
    let url = format!(
        "https://cdn.syndication.twimg.com/tweet-result?id={tweet_id}&token={token}&lang=en"
    );

    Client::builder()
        .user_agent("guenther/0.1.0")
        .build()
        .map_err(|e| Error::other(format!("failed to build reqwest client: {e}")))?
        .get(url)
        .send()
        .await
        .map_err(|e| Error::other(format!("failed to fetch twitter syndication data: {e}")))?
        .error_for_status()
        .map_err(|e| Error::other(format!("failed to fetch twitter syndication data: {e}")))?
        .json::<Value>()
        .await
        .map_err(|e| Error::other(format!("failed to parse twitter syndication data: {e}")))
}

fn is_no_video_tweet_error(err: &Error) -> bool {
    matches!(err, Error::YTDLPFailed(message) if message.contains("No video could be found in this tweet"))
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

fn parse_twitter_post_text(content: &str) -> Option<String> {
    let json = serde_json::from_str::<Value>(content).ok()?;
    parse_twitter_post_text_from_value(&json)
}

fn parse_twitter_post_text_from_value(json: &Value) -> Option<String> {
    let text = json
        .get("full_text")
        .and_then(Value::as_str)
        .or_else(|| json.get("text").and_then(Value::as_str))
        .or_else(|| json.get("description").and_then(Value::as_str))
        .or_else(|| json.get("title").and_then(Value::as_str))?
        .trim();

    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_prefers_full_text() {
        let content = r#"{"full_text":"hello world","description":"fallback"}"#;
        assert_eq!(
            parse_twitter_post_text(content).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn parse_falls_back_to_description() {
        let content = r#"{"description":"fallback"}"#;
        assert_eq!(
            parse_twitter_post_text(content).as_deref(),
            Some("fallback")
        );
    }

    #[test]
    fn parse_rejects_empty_text() {
        let content = r#"{"full_text":"   "}"#;
        assert!(parse_twitter_post_text(content).is_none());
    }

    #[test]
    fn parse_falls_back_to_text() {
        let content = r#"{"text":"tweet body"}"#;
        assert_eq!(
            parse_twitter_post_text(content).as_deref(),
            Some("tweet body")
        );
    }

    #[test]
    fn extracts_tweet_id_from_status_url() {
        assert_eq!(
            extract_tweet_id("https://x.com/i/status/2037144468625801425").as_deref(),
            Some("2037144468625801425")
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
        assert!(!syndication_token("2037144468625801425").is_empty());
    }
}
