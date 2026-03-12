use crate::{
    error::{Error, Result},
    utils::{MediaKind, detect_media_kind_async, send_media_from_path},
};
use futures::{StreamExt, stream};
use std::{cmp::min, path::PathBuf};
use teloxide::{Bot, types::ChatId};
use tempfile::TempDir;
use tracing::debug;

/// `TempDir` guard + downloaded files. Keep this value alive until you're
/// done sending files so the temporary directory is not deleted.
#[derive(Debug)]
pub struct DownloadResult {
    pub tempdir: TempDir,
    pub files: Vec<PathBuf>,
}

/// Post-process a `DownloadResult`.
///
/// Detect media kinds (async), prefer video, then image, then call `send_media_from_path`.
/// Keeps the tempdir alive while sending because `DownloadResult` is passed by value.
///
/// # Errors
///
/// - Propagates `send_media_from_path` errors or returns NoMediaFound/UnknownMediaKind.
pub async fn process_download_result(
    bot: &Bot,
    chat_id: ChatId,
    mut dr: DownloadResult,
) -> Result<()> {
    debug!(files = dr.files.len(), "Processing download result");

    if dr.files.is_empty() {
        return Err(Error::NoMediaFound);
    }

    // Detect kinds and validate files in parallel
    let concurrency = min(8, dr.files.len());
    let results = stream::iter(dr.files.drain(..).map(|path| async move {
        // Check file metadata asynchronously
        let Ok(meta) = tokio::fs::metadata(&path).await else {
            return None;
        };
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }

        let kind = detect_media_kind_async(&path).await;
        match kind {
            MediaKind::Unknown => None,
            k => Some((path, k)),
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;

    let mut media_items = results.into_iter().flatten().collect::<Vec<_>>();

    if media_items.is_empty() {
        return Err(Error::NoMediaFound);
    }

    // deterministic ordering
    media_items.sort_by_key(|(path, kind)| {
        let priority = match kind {
            MediaKind::Video => 0,
            MediaKind::Image => 1,
            MediaKind::Unknown => 2,
        };
        (priority, path.clone())
    });

    debug!(media_items = media_items.len(), "Sending media to chat");

    for (path, kind) in media_items {
        send_media_from_path(bot, chat_id, path, kind).await?;
    }
    Ok(())
}
