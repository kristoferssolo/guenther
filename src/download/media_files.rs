use crate::{
    error::{Error, Result},
    utils::{MediaKind, detect_media_kind},
};
use std::path::PathBuf;
use tempfile::TempDir;

/// `TempDir` guard + downloaded files. Keep this value alive until you're
/// done sending files so the temporary directory is not deleted.
#[derive(Debug)]
pub struct DownloadResult {
    pub tempdir: TempDir,
    pub files: Vec<PathBuf>,
    pub source_text: Option<String>,
}

impl DownloadResult {
    /// Classify the downloaded files, keeping non-empty images and videos in
    /// download order.
    ///
    /// The returned paths point into the tempdir owned by `self`, so keep the
    /// `DownloadResult` alive until the files have been sent.
    ///
    /// # Errors
    ///
    /// Returns `NoMediaFound` when there are no valid image/video files left after classification.
    pub async fn supported_media(&self) -> Result<Vec<(PathBuf, MediaKind)>> {
        let mut media_items = Vec::new();
        for path in &self.files {
            let Ok(meta) = tokio::fs::metadata(path).await else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            if let Some(kind) = detect_media_kind(path).await {
                media_items.push((path.clone(), kind));
            }
        }

        if media_items.is_empty() {
            return Err(Error::NoMediaFound);
        }
        Ok(media_items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;

    #[tokio::test]
    async fn supported_media_preserves_download_order_for_mixed_media() {
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
        let media_items = assert_ok!(result.supported_media().await);

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
