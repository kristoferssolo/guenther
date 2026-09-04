use crate::{
    config::CobaltConfig,
    download::DownloadResult,
    error::{Error, Result},
};
use futures::StreamExt;
use reqwest::{
    Client,
    header::{ACCEPT, AUTHORIZATION},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::{TempDir, tempdir};
use tokio::{fs::File, io::AsyncWriteExt};
use tracing::debug;

const REQUEST_TIMEOUT: Duration = Duration::from_mins(5);

#[derive(Debug, Clone)]
pub(super) struct CobaltClient {
    http: Client,
    config: CobaltConfig,
}

impl CobaltClient {
    pub(super) fn new(config: CobaltConfig) -> Result<Self> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(Error::BuildHttpClient)?;
        Ok(Self { http, config })
    }

    pub(super) async fn download(&self, source_url: &str) -> Result<DownloadResult> {
        let response = self.request_download(source_url).await?;
        let tempdir = tempdir()?;

        let files = match response {
            CobaltResponse::Tunnel { url, filename }
            | CobaltResponse::Redirect { url, filename } => {
                vec![self.download_media(&tempdir, &url, &filename, 0).await?]
            }
            CobaltResponse::Picker { picker } => {
                let mut files = Vec::with_capacity(picker.len());
                for (index, item) in picker.into_iter().enumerate() {
                    let filename = format!("media-{index}.{}", extension_for(&item.media_type));
                    files.push(
                        self.download_media(&tempdir, &item.url, &filename, index)
                            .await?,
                    );
                }
                files
            }
            CobaltResponse::LocalProcessing => return Err(Error::CobaltLocalProcessing),
            CobaltResponse::Rejected { error } => return Err(Error::CobaltRejected(error.code)),
        };

        if files.is_empty() {
            return Err(Error::NoMediaFound);
        }

        Ok(DownloadResult {
            tempdir,
            files,
            source_text: None,
        })
    }

    async fn request_download(&self, source_url: &str) -> Result<CobaltResponse> {
        let payload = CobaltRequest {
            url: source_url,
            video_quality: "1080",
            youtube_video_codec: "h264",
            youtube_video_container: "mp4",
            convert_gif: false,
            filename_style: "basic",
            always_proxy: true,
            local_processing: "disabled",
        };
        let mut request = self
            .http
            .post(&self.config.api_url)
            .header(ACCEPT, "application/json")
            .json(&payload);
        if let Some(api_key) = &self.config.api_key {
            request = request.header(AUTHORIZATION, format!("Api-Key {api_key}"));
        }

        debug!("Requesting Cobalt download");
        let response = request.send().await.map_err(Error::CobaltRequest)?;
        debug!(status = %response.status(), "Received Cobalt response");
        response.json().await.map_err(Error::CobaltRequest)
    }

    async fn download_media(
        &self,
        tempdir: &TempDir,
        media_url: &str,
        suggested_filename: &str,
        index: usize,
    ) -> Result<PathBuf> {
        let filename = safe_filename(suggested_filename, index);
        let path = tempdir.path().join(filename);
        let response = self
            .http
            .get(media_url)
            .send()
            .await
            .map_err(Error::CobaltRequest)?;
        debug!(index, status = %response.status(), "Received Cobalt media response");
        let response = response.error_for_status().map_err(Error::CobaltRequest)?;
        let mut stream = response.bytes_stream();
        let mut file = File::create(&path).await?;

        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk.map_err(Error::CobaltRequest)?)
                .await?;
        }
        file.flush().await?;

        Ok(path)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CobaltRequest<'a> {
    url: &'a str,
    video_quality: &'static str,
    youtube_video_codec: &'static str,
    youtube_video_container: &'static str,
    convert_gif: bool,
    filename_style: &'static str,
    always_proxy: bool,
    local_processing: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status")]
enum CobaltResponse {
    #[serde(rename = "tunnel")]
    Tunnel { url: String, filename: String },
    #[serde(rename = "redirect")]
    Redirect { url: String, filename: String },
    #[serde(rename = "picker")]
    Picker { picker: Vec<PickerItem> },
    #[serde(rename = "local-processing")]
    LocalProcessing,
    #[serde(rename = "error")]
    Rejected { error: CobaltApiError },
}

#[derive(Debug, Deserialize)]
struct PickerItem {
    #[serde(rename = "type")]
    media_type: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct CobaltApiError {
    code: String,
}

fn safe_filename(suggested: &str, index: usize) -> String {
    let basename = Path::new(suggested)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .unwrap_or("media");
    format!("{index:03}-{basename}")
}

fn extension_for(media_type: &str) -> &'static str {
    match media_type {
        "photo" => "jpg",
        "video" => "mp4",
        "gif" => "gif",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_matches, assert_ok, assert_some};
    use serde_json::{Value, from_str, to_value};

    #[test]
    fn serializes_cobalt_request_with_api_field_names() {
        let request = CobaltRequest {
            url: "https://www.youtube.com/shorts/example",
            video_quality: "1080",
            youtube_video_codec: "h264",
            youtube_video_container: "mp4",
            convert_gif: false,
            filename_style: "basic",
            always_proxy: true,
            local_processing: "disabled",
        };

        let value = assert_ok!(to_value(request));
        assert_eq!(
            value.get("youtubeVideoCodec"),
            Some(&Value::String("h264".into()))
        );
        assert_eq!(value.get("convertGif"), Some(&Value::Bool(false)));
        assert_eq!(value.get("alwaysProxy"), Some(&Value::Bool(true)));
        assert_eq!(
            value.get("localProcessing"),
            Some(&Value::String("disabled".into()))
        );
    }

    #[test]
    fn deserializes_picker_response() {
        let response = assert_ok!(from_str::<CobaltResponse>(
            r#"{"status":"picker","picker":[{"type":"photo","url":"https://example.com/a"}]}"#,
        ));

        assert_matches!(&response, CobaltResponse::Picker { .. });
        let CobaltResponse::Picker { picker } = response else {
            return;
        };
        assert_eq!(picker.len(), 1);
        assert_eq!(assert_some!(picker.first()).media_type, "photo");
    }

    #[test]
    fn deserializes_error_response() {
        let response = assert_ok!(from_str::<CobaltResponse>(
            r#"{"status":"error","error":{"code":"error.api.youtube.login"}}"#,
        ));

        assert_matches!(&response, CobaltResponse::Rejected { .. });
        let CobaltResponse::Rejected { error } = response else {
            return;
        };
        assert_eq!(error.code, "error.api.youtube.login");
    }

    #[test]
    fn strips_directories_from_suggested_filename() {
        assert_eq!(safe_filename("../../video.mp4", 2), "002-video.mp4");
        assert_eq!(safe_filename("..", 0), "000-media");
    }
}
