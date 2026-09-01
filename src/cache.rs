use crate::utils::MediaKind;
use sqlx::SqlitePool;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database contains invalid media kind `{0}`")]
    InvalidMediaKind(String),

    #[error("Media position `{0}` does not fit in the database")]
    PositionOutOfRange(usize),
}

pub type Result<T> = std::result::Result<T, CacheError>;

/// A media item Telegram can serve by `file_id` alone, without re-uploading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedMedia {
    pub kind: MediaKind,
    pub file_id: String,
}

/// Persistent store of Telegram `file_id`s keyed by the exact source URL.
///
/// Rows are ordered by upload position; captions are never cached.
#[derive(Debug, Clone)]
pub struct MediaCache {
    pool: SqlitePool,
}

impl MediaCache {
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Return the cached media for `url`, ordered by upload position.
    ///
    /// Unknown URLs (and entries stored with no items) map to `None`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails or a stored row has an
    /// invalid `kind`.
    pub async fn get(&self, url: &str) -> Result<Option<Vec<CachedMedia>>> {
        let rows = sqlx::query!(
            r#"SELECT kind, file_id FROM media_cache WHERE url = ? ORDER BY position"#,
            url
        )
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| {
                Ok(CachedMedia {
                    kind: media_kind_from_db(&row.kind)?,
                    file_id: row.file_id,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((!items.is_empty()).then_some(items))
    }

    /// Replace the cached media for `url` with `items`, positionally ordered.
    ///
    /// Empty item lists are ignored: an album that produced nothing is not a
    /// cache hit.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction fails or an item is not a
    /// storable `video`/`image` kind.
    pub async fn put(&self, url: &str, items: &[CachedMedia]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query!(r#"DELETE FROM media_cache WHERE url = ?"#, url)
            .execute(&mut *transaction)
            .await?;
        for (index, item) in items.iter().enumerate() {
            let position =
                i64::try_from(index).map_err(|_| CacheError::PositionOutOfRange(index))?;
            let kind = match item.kind {
                MediaKind::Video => "video",
                MediaKind::Image => "image",
                MediaKind::Unknown => {
                    return Err(CacheError::InvalidMediaKind(
                        MediaKind::Unknown.to_str().to_owned(),
                    ));
                }
            };
            sqlx::query!(
                r#"INSERT INTO media_cache (url, position, kind, file_id) VALUES (?, ?, ?, ?)"#,
                url,
                position,
                kind,
                item.file_id
            )
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Remove the cached media for `url`, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails.
    pub async fn invalidate(&self, url: &str) -> Result<()> {
        sqlx::query!(r#"DELETE FROM media_cache WHERE url = ?"#, url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Only `video` and `image` pass the table's `CHECK` constraint; anything else
/// means the stored row is corrupt and should surface as an error.
fn media_kind_from_db(kind: &str) -> Result<MediaKind> {
    match kind {
        "video" => Ok(MediaKind::Video),
        "image" => Ok(MediaKind::Image),
        other => Err(CacheError::InvalidMediaKind(other.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use claims::{assert_none, assert_ok, assert_some_eq};

    async fn cache() -> MediaCache {
        let pool = assert_ok!(db::connect("sqlite::memory:").await);
        MediaCache::new(pool)
    }

    fn media(file_id: &str, kind: MediaKind) -> CachedMedia {
        CachedMedia {
            kind,
            file_id: file_id.to_owned(),
        }
    }

    #[tokio::test]
    async fn unknown_url_is_not_cached() {
        let cache = cache().await;
        assert_none!(assert_ok!(cache.get("https://example.com/none").await));
    }

    #[tokio::test]
    async fn roundtrip_preserves_order_and_kinds() {
        let cache = cache().await;
        let items = vec![
            media("video-id", MediaKind::Video),
            media("image-id", MediaKind::Image),
        ];
        assert_ok!(cache.put("https://example.com/a", &items).await);
        assert_some_eq!(assert_ok!(cache.get("https://example.com/a").await), items);
    }

    #[tokio::test]
    async fn put_replaces_previous_items() {
        let cache = cache().await;
        let url = "https://example.com/b";
        assert_ok!(cache.put(url, &[media("old", MediaKind::Video)]).await);
        let items = vec![
            media("new-1", MediaKind::Image),
            media("new-2", MediaKind::Image),
        ];
        assert_ok!(cache.put(url, &items).await);
        assert_some_eq!(assert_ok!(cache.get(url).await), items);
    }

    #[tokio::test]
    async fn invalidate_removes_entry() {
        let cache = cache().await;
        let url = "https://example.com/c";
        assert_ok!(cache.put(url, &[media("id", MediaKind::Video)]).await);
        assert_ok!(cache.invalidate(url).await);
        assert_none!(assert_ok!(cache.get(url).await));
    }

    #[tokio::test]
    async fn empty_item_lists_are_not_stored() {
        let cache = cache().await;
        let url = "https://example.com/d";
        assert_ok!(cache.put(url, &[]).await);
        assert_none!(assert_ok!(cache.get(url).await));
    }
}
