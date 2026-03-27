use crate::utils::{MediaKind, detect_media_kind_async};
use futures::{StreamExt, stream};
use std::{cmp::min, path::PathBuf};
use tempfile::TempDir;

/// `TempDir` guard + downloaded files. Keep this value alive until you're
/// done sending files so the temporary directory is not deleted.
#[derive(Debug)]
pub struct DownloadResult {
    pub tempdir: TempDir,
    pub files: Vec<PathBuf>,
    pub source_text: Option<String>,
}

/// Classify and filter files in a `DownloadResult`.
///
/// Keeps the tempdir alive while the returned `DownloadResult` remains in scope.
///
/// # Errors
///
/// Returns `NoMediaFound` when there are no valid image/video files left after classification.
pub async fn collect_supported_media(
    mut dr: DownloadResult,
) -> crate::error::Result<(TempDir, Vec<(PathBuf, MediaKind)>)> {
    if dr.files.is_empty() {
        return Err(crate::error::Error::NoMediaFound);
    }

    let concurrency = min(8, dr.files.len());
    let results = stream::iter(dr.files.drain(..).map(|path| async move {
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
        return Err(crate::error::Error::NoMediaFound);
    }

    media_items.sort_by_key(|(path, kind)| {
        let priority = match kind {
            MediaKind::Video => 0,
            MediaKind::Image => 1,
            MediaKind::Unknown => 2,
        };
        (priority, path.clone())
    });

    Ok((dr.tempdir, media_items))
}
