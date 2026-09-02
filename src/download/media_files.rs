use crate::{
    error::{Error, Result},
    utils::{MediaKind, detect_media_kind_async},
};
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
) -> Result<(TempDir, Vec<(PathBuf, MediaKind)>)> {
    if dr.files.is_empty() {
        return Err(Error::NoMediaFound);
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
    .buffered(concurrency)
    .collect::<Vec<_>>()
    .await;

    let media_items = results.into_iter().flatten().collect::<Vec<_>>();
    if media_items.is_empty() {
        return Err(Error::NoMediaFound);
    }

    Ok((dr.tempdir, media_items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;

    #[tokio::test]
    async fn collect_preserves_download_order_for_mixed_media() {
        let tempdir = assert_ok!(tempfile::tempdir());
        let image = tempdir.path().join("first.jpg");
        let video = tempdir.path().join("second.mp4");
        let second_image = tempdir.path().join("third.png");
        assert_ok!(std::fs::write(&image, b"image"));
        assert_ok!(std::fs::write(&video, b"video"));
        assert_ok!(std::fs::write(&second_image, b"image"));

        let result = DownloadResult {
            tempdir,
            files: vec![image.clone(), video.clone(), second_image.clone()],
            source_text: None,
        };
        let (_, media_items) = assert_ok!(collect_supported_media(result).await);

        assert_eq!(
            media_items,
            vec![
                (image, MediaKind::Image),
                (video, MediaKind::Video),
                (second_image, MediaKind::Image),
            ]
        );
    }
}
