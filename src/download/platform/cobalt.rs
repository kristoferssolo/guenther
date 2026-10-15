use crate::{
    config::CobaltConfig,
    download::{
        DownloadResult,
        error::{DownloadError, Result},
    },
};
use futures::{StreamExt, TryStreamExt, stream};
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
/// Caps parallel requests to Cobalt and the source site for large carousels.
const PICKER_CONCURRENCY: usize = 4;

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
            .map_err(DownloadError::BuildHttpClient)?;
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
            CobaltResponse::Picker { picker } => self.download_picker(&tempdir, picker).await?,
            CobaltResponse::LocalProcessing => return Err(DownloadError::CobaltLocalProcessing),
            CobaltResponse::Rejected { error } => {
                return Err(DownloadError::CobaltRejected(error.code));
            }
        };

        if files.is_empty() {
            return Err(DownloadError::NoMediaFound);
        }

        Ok(DownloadResult {
            tempdir,
            files,
            source_text: None,
        })
    }

    async fn request_download(&self, source_url: &str) -> Result<CobaltResponse> {
        let mut request = self
            .http
            .post(&self.config.api_url)
            .header(ACCEPT, "application/json")
            .json(&CobaltRequest::new(source_url));
        if let Some(api_key) = &self.config.api_key {
            request = request.header(AUTHORIZATION, format!("Api-Key {api_key}"));
        }

        debug!("Requesting Cobalt download");
        let response = request.send().await.map_err(DownloadError::CobaltRequest)?;
        debug!(status = %response.status(), "Received Cobalt response");
        response.json().await.map_err(DownloadError::CobaltRequest)
    }

    /// Downloads picker items concurrently, returning paths in picker order.
    async fn download_picker(
        &self,
        tempdir: &TempDir,
        picker: Vec<PickerItem>,
    ) -> Result<Vec<PathBuf>> {
        stream::iter(picker.into_iter().enumerate())
            .map(|(index, item)| async move {
                let filename = format!("media-{index}.{}", extension_for(&item.media_type));
                self.download_media(tempdir, &item.url, &filename, index)
                    .await
            })
            .buffered(PICKER_CONCURRENCY)
            .try_collect()
            .await
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
            .map_err(DownloadError::CobaltRequest)?;
        debug!(index, status = %response.status(), "Received Cobalt media response");
        let response = response
            .error_for_status()
            .map_err(DownloadError::CobaltRequest)?;
        let mut stream = response.bytes_stream();
        let mut file = File::create(&path).await?;

        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk.map_err(DownloadError::CobaltRequest)?)
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

impl<'a> CobaltRequest<'a> {
    const fn new(url: &'a str) -> Self {
        Self {
            url,
            video_quality: "1080",
            youtube_video_codec: "h264",
            youtube_video_container: "mp4",
            convert_gif: false,
            filename_style: "basic",
            always_proxy: true,
            local_processing: "disabled",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
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

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct PickerItem {
    #[serde(rename = "type")]
    media_type: String,
    url: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
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
    use claims::{assert_ok, assert_ok_eq, assert_some};
    use rstest::rstest;
    use serde_json::{from_str, json, to_value};
    use std::fs;
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        net::{TcpListener, TcpStream},
        time::sleep,
    };

    /// Serves each request path back as its body, delaying `/slow`.
    async fn serve_paths(listener: TcpListener) {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(respond_with_path(socket));
        }
    }

    async fn respond_with_path(socket: TcpStream) {
        let mut reader = BufReader::new(socket);
        let mut request_line = String::new();
        assert_ok!(reader.read_line(&mut request_line).await);
        let (path, _) = assert_some!(
            request_line
                .strip_prefix("GET ")
                .and_then(|rest| rest.split_once(' '))
        );

        let mut header = String::new();
        loop {
            header.clear();
            let read = assert_ok!(reader.read_line(&mut header).await);
            if read == 0 || header == "\r\n" {
                break;
            }
        }

        if path == "/slow" {
            sleep(Duration::from_millis(200)).await;
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{path}",
            path.len()
        );
        assert_ok!(reader.get_mut().write_all(response.as_bytes()).await);
    }

    #[test]
    fn serializes_cobalt_request_with_api_field_names() {
        let url = "https://www.youtube.com/shorts/example";

        assert_ok_eq!(
            to_value(CobaltRequest::new(url)),
            json!({
                "url": url,
                "videoQuality": "1080",
                "youtubeVideoCodec": "h264",
                "youtubeVideoContainer": "mp4",
                "convertGif": false,
                "filenameStyle": "basic",
                "alwaysProxy": true,
                "localProcessing": "disabled",
            })
        );
    }

    #[rstest]
    #[case::picker(
        r#"{"status":"picker","picker":[{"type":"photo","url":"https://example.com/a"}]}"#,
        CobaltResponse::Picker {
            picker: vec![PickerItem {
                media_type: "photo".into(),
                url: "https://example.com/a".into(),
            }],
        },
    )]
    #[case::error(
        r#"{"status":"error","error":{"code":"error.api.youtube.login"}}"#,
        CobaltResponse::Rejected {
            error: CobaltApiError {
                code: "error.api.youtube.login".into(),
            },
        },
    )]
    fn deserializes_response(#[case] raw: &str, #[case] expected: CobaltResponse) {
        assert_ok_eq!(from_str::<CobaltResponse>(raw), expected);
    }

    #[test]
    fn strips_directories_from_suggested_filename() {
        assert_eq!(safe_filename("../../video.mp4", 2), "002-video.mp4");
        assert_eq!(safe_filename("..", 0), "000-media");
    }

    #[tokio::test]
    async fn downloads_picker_items_in_picker_order() {
        let listener = assert_ok!(TcpListener::bind("127.0.0.1:0").await);
        let address = assert_ok!(listener.local_addr());
        tokio::spawn(serve_paths(listener));

        let client = assert_ok!(CobaltClient::new(CobaltConfig::default()));
        let tempdir = assert_ok!(tempdir());
        let bodies = ["/slow", "/fast"];
        let picker = bodies
            .map(|path| PickerItem {
                media_type: "photo".into(),
                url: format!("http://{address}{path}"),
            })
            .into();

        let files = assert_ok!(client.download_picker(&tempdir, picker).await);

        assert_eq!(
            files,
            ["000-media-0.jpg", "001-media-1.jpg"].map(|name| tempdir.path().join(name))
        );
        for (file, body) in files.iter().zip(bodies) {
            assert_ok_eq!(fs::read_to_string(file), body);
        }
    }
}
