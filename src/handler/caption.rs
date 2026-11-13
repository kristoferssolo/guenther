use crate::{
    cache::CachedMedia,
    config::Platform,
    download::DownloadResult,
    handler::delivery::compose_caption,
    llm::{CaptionRequest, Captioner, LlmError, LocalMedia},
    telegram_file::{TelegramFileError, TelegramFiles},
    utils::MediaKind,
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use teloxide::{
    Bot, RequestError,
    prelude::*,
    types::{ChatId, FileId, MessageId},
};
use tempfile::tempdir;
use thiserror::Error;
use tokio::{
    task::{JoinError, JoinHandle},
    time::timeout,
};
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
    Download(#[from] TelegramFileError),

    #[error("Caption task stopped: {0}")]
    Join(#[from] JoinError),
}

/// Where a caption job finds the media the model looks at.
#[derive(Debug)]
pub(super) enum JobMedia {
    /// A fresh download, shared with the sender so its tempdir lives until
    /// both the upload and the caption are done.
    Downloaded {
        download: Arc<DownloadResult>,
        items: Vec<(PathBuf, MediaKind)>,
    },
    /// A cache hit; the planned items are fetched back from Telegram.
    Cached(Vec<CachedMedia>),
}

/// Inputs for one LLM caption.
#[derive(Debug)]
pub(super) struct CaptionJob {
    pub captioner: Captioner,
    pub bot: Bot,
    pub files: TelegramFiles,
    pub platform: Platform,
    pub sender_name: Option<String>,
    pub message_text: Option<String>,
    pub media: JobMedia,
}

impl CaptionJob {
    /// Start generating the caption in the background.
    pub(super) fn start(self) -> PendingCaption {
        PendingCaption {
            platform: self.platform,
            started_at: Instant::now(),
            handle: tokio::spawn(self.generate()),
        }
    }

    async fn generate(self) -> Result<String, CaptionJobError> {
        let mut request = CaptionRequest {
            platform: self.platform,
            sender_name: self.sender_name,
            message_text: self.message_text,
            source_text: None,
            media: Vec::new(),
        };

        match self.media {
            JobMedia::Downloaded { download, items } => {
                request.source_text.clone_from(&download.source_text);
                request.media = items
                    .into_iter()
                    .map(|(path, kind)| LocalMedia { path, kind })
                    .collect();
                Ok(self.captioner.caption(&request).await?)
            }
            JobMedia::Cached(items) => {
                let fetch_dir = tempdir()?;
                let count = self
                    .captioner
                    .visual_item_count(items.iter().map(|item| item.kind));
                request.media = fetch_cached(
                    &self.bot,
                    &self.files,
                    items.into_iter().take(count),
                    fetch_dir.path(),
                )
                .await;
                Ok(self.captioner.caption(&request).await?)
            }
        }
    }
}

/// What to caption the media with right now.
#[derive(Debug)]
pub(super) enum InitialCaption {
    /// The LLM caption arrived in time.
    Ready(String),
    /// Still generating; send a static caption and apply this one later.
    Late(PendingCaption),
    /// No LLM caption will come; keep the static caption.
    Unavailable,
}

impl InitialCaption {
    /// Split into the caption to send now and a late LLM caption to apply
    /// after sending, if any.
    pub(super) fn or_static(
        self,
        static_caption: impl FnOnce() -> String,
    ) -> (String, Option<PendingCaption>) {
        match self {
            Self::Ready(line) => (line, None),
            Self::Late(pending) => (static_caption(), Some(pending)),
            Self::Unavailable => (static_caption(), None),
        }
    }
}

/// A caption still being generated. Dropping it cancels the generation.
#[derive(Debug)]
pub(super) struct PendingCaption {
    platform: Platform,
    started_at: Instant,
    handle: JoinHandle<Result<String, CaptionJobError>>,
}

impl PendingCaption {
    /// Wait up to `limit` for the caption.
    pub(super) async fn wait(mut self, limit: Duration) -> InitialCaption {
        let Ok(joined) = timeout(limit, &mut self.handle).await else {
            info!(
                platform = %self.platform,
                ?limit,
                "LLM caption is late; sending with a static caption"
            );
            return InitialCaption::Late(self);
        };
        match joined
            .map_err(CaptionJobError::from)
            .and_then(|result| result)
        {
            Ok(line) => {
                info!(
                    platform = %self.platform,
                    elapsed = ?self.started_at.elapsed(),
                    "Sending media with LLM caption"
                );
                InitialCaption::Ready(line)
            }
            Err(err) => {
                warn!(
                    platform = %self.platform,
                    %err,
                    "LLM caption failed; using a static caption"
                );
                InitialCaption::Unavailable
            }
        }
    }

    /// Replace the caption of `message_ids` once the LLM caption arrives,
    /// appending `appended_text` like the static caption did. Failures only
    /// log and keep the static caption.
    pub(super) fn apply_later(
        mut self,
        bot: Bot,
        chat_id: ChatId,
        message_ids: Vec<MessageId>,
        appended_text: Option<String>,
    ) {
        tokio::spawn(async move {
            let applied = async {
                let line = (&mut self.handle).await??;
                let caption = compose_caption(&line, appended_text.as_deref());
                for message_id in message_ids {
                    bot.edit_message_caption(chat_id, message_id)
                        .caption(caption.clone())
                        .await?;
                }
                Ok::<_, CaptionJobError>(())
            }
            .await;
            match applied {
                Ok(()) => info!(
                    platform = %self.platform,
                    elapsed = ?self.started_at.elapsed(),
                    "Replaced static caption with late LLM caption"
                ),
                Err(err) => warn!(
                    platform = %self.platform,
                    %err,
                    "Late LLM caption failed; keeping the static caption"
                ),
            }
        });
    }
}

impl Drop for PendingCaption {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Download cached media back from Telegram, skipping items that fail (the
/// cloud Bot API refuses files over 20 MB).
async fn fetch_cached(
    bot: &Bot,
    files: &TelegramFiles,
    items: impl IntoIterator<Item = CachedMedia>,
    dir: &Path,
) -> Vec<LocalMedia> {
    let mut media = Vec::new();
    for (number, item) in (1_usize..).zip(items) {
        let path = dir.join(format!("cached-{number}"));
        match fetch_file(bot, files, item.file_id, &path).await {
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

async fn fetch_file(
    bot: &Bot,
    files: &TelegramFiles,
    file_id: String,
    path: &Path,
) -> Result<(), CaptionJobError> {
    let file = bot.get_file(FileId(file_id)).await?;
    files.save(bot, &file.path, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use std::{assert_matches, future};
    use tokio::task::yield_now;

    fn pending_caption(
        generation: impl Future<Output = Result<String, CaptionJobError>> + Send + 'static,
    ) -> PendingCaption {
        PendingCaption {
            platform: Platform::Twitter,
            started_at: Instant::now(),
            handle: tokio::spawn(generation),
        }
    }

    #[tokio::test]
    async fn caption_finished_within_the_wait_is_sent_directly() {
        let initial = pending_caption(async { Ok("Box. Now.".to_owned()) })
            .wait(Duration::from_secs(5))
            .await;
        assert_matches!(initial, InitialCaption::Ready(line) if line == "Box. Now.");
    }

    #[tokio::test]
    async fn caption_still_running_after_the_wait_is_late() {
        let initial = pending_caption(future::pending())
            .wait(Duration::from_millis(10))
            .await;
        assert_matches!(initial, InitialCaption::Late(_));
    }

    #[tokio::test]
    async fn failed_caption_falls_back_to_static() {
        let initial = pending_caption(async { Err(LlmError::EmptyResponse.into()) })
            .wait(Duration::from_secs(5))
            .await;
        assert_matches!(initial, InitialCaption::Unavailable);
    }

    #[tokio::test]
    async fn dropping_a_pending_caption_cancels_generation() {
        let pending = pending_caption(future::pending());
        let generation = pending.handle.abort_handle();
        drop(pending);

        assert_ok!(
            timeout(Duration::from_secs(1), async {
                while !generation.is_finished() {
                    yield_now().await;
                }
            })
            .await
        );
    }
}
