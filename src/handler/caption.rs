use crate::{
    cache::CachedMedia,
    config::Platform,
    download::DownloadResult,
    handler::delivery::compose_caption,
    llm::{CaptionRequest, Captioner, LlmError, LocalMedia},
    utils::MediaKind,
};
use std::{
    io,
    path::{Path, PathBuf},
    time::Instant,
};
use teloxide::{
    Bot, DownloadError as TelegramDownloadError, RequestError,
    net::Download,
    prelude::*,
    types::{ChatId, FileId, MessageId},
};
use tempfile::tempdir;
use thiserror::Error;
use tokio::{fs::File, io::AsyncWriteExt};
use tracing::{info, warn};

#[derive(Debug, Error)]
enum CaptionJobError {
    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] RequestError),

    #[error("Telegram file download failed: {0}")]
    Download(#[from] TelegramDownloadError),
}

/// Where a caption job finds the media the model looks at.
#[derive(Debug)]
pub(super) enum JobMedia {
    /// A fresh download. The job owns it so its tempdir outlives the request.
    Downloaded {
        download: DownloadResult,
        items: Vec<(PathBuf, MediaKind)>,
        /// Whether the sent caption appends the post text, as the static one did.
        append_source_text: bool,
    },
    /// A cache hit; the planned items are fetched back from Telegram.
    Cached(Vec<CachedMedia>),
}

/// Replaces the static caption on already-sent media with an LLM caption.
#[derive(Debug)]
pub(super) struct CaptionJob {
    pub captioner: Captioner,
    pub bot: Bot,
    pub chat_id: ChatId,
    pub message_ids: Vec<MessageId>,
    pub platform: Platform,
    pub sender_name: Option<String>,
    pub message_text: Option<String>,
    pub media: JobMedia,
}

impl CaptionJob {
    /// Run in the background; failures only log and keep the static caption.
    pub(super) fn spawn(self) {
        tokio::spawn(async move {
            let started_at = Instant::now();
            let platform = self.platform;
            match self.run().await {
                Ok(()) => info!(
                    %platform,
                    elapsed = ?started_at.elapsed(),
                    "Replaced caption with LLM caption"
                ),
                Err(err) => warn!(
                    %platform,
                    %err,
                    "LLM caption failed; keeping the static caption"
                ),
            }
        });
    }

    async fn run(self) -> Result<(), CaptionJobError> {
        let mut request = CaptionRequest {
            platform: self.platform,
            sender_name: self.sender_name,
            message_text: self.message_text,
            source_text: None,
            media: Vec::new(),
        };

        let (line, appended_text) = match self.media {
            JobMedia::Downloaded {
                download,
                items,
                append_source_text,
            } => {
                request.source_text.clone_from(&download.source_text);
                request.media = items
                    .into_iter()
                    .map(|(path, kind)| LocalMedia { path, kind })
                    .collect();
                let line = self.captioner.caption(&request).await?;
                let appended_text = append_source_text
                    .then(|| download.source_text.clone())
                    .flatten();
                (line, appended_text)
            }
            JobMedia::Cached(items) => {
                let fetch_dir = tempdir()?;
                let count = self
                    .captioner
                    .visual_item_count(items.iter().map(|item| item.kind));
                request.media =
                    fetch_cached(&self.bot, items.into_iter().take(count), fetch_dir.path()).await;
                (self.captioner.caption(&request).await?, None)
            }
        };

        let caption = compose_caption(&line, appended_text.as_deref());
        for message_id in self.message_ids {
            self.bot
                .edit_message_caption(self.chat_id, message_id)
                .caption(caption.clone())
                .await?;
        }
        Ok(())
    }
}

/// Download cached media back from Telegram, skipping items that fail (the
/// Bot API refuses files over 20 MB).
async fn fetch_cached(
    bot: &Bot,
    items: impl IntoIterator<Item = CachedMedia>,
    dir: &Path,
) -> Vec<LocalMedia> {
    let mut media = Vec::new();
    for (number, item) in (1_usize..).zip(items) {
        let path = dir.join(format!("cached-{number}"));
        match fetch_file(bot, item.file_id, &path).await {
            Ok(()) => media.push(LocalMedia {
                path,
                kind: item.kind,
            }),
            Err(err) => {
                warn!(%err, item = number, "Failed to fetch cached media for the LLM caption");
            }
        }
    }
    media
}

async fn fetch_file(bot: &Bot, file_id: String, path: &Path) -> Result<(), CaptionJobError> {
    let file = bot.get_file(FileId(file_id)).await?;
    let mut destination = File::create(path).await?;
    bot.download_file(&file.path, &mut destination).await?;
    destination.flush().await?;
    Ok(())
}
