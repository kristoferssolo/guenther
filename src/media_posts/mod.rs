mod period;
mod report;

pub use crate::media_posts::{
    period::StatsPeriod,
    report::{LinkStats, ShareCount},
};

use crate::handler::MediaLink;
use chrono::{DateTime, Utc};
use sqlx::{SqliteConnection, SqlitePool};
use teloxide::types::{Chat, ChatId, Message, MessageId};
use thiserror::Error;

const TOP_SENDERS: i64 = 10;
const TOP_LINKS: i64 = 5;

#[derive(Debug, Error)]
pub enum MediaPostsError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Telegram user id `{0}` does not fit in the database")]
    UserIdOutOfRange(u64),
}

pub type Result<T> = std::result::Result<T, MediaPostsError>;

/// One successfully delivered media link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPost {
    pub chat_id: ChatId,
    /// The message that contained the link.
    pub message_id: MessageId,
    /// Telegram user id, or the (negative) chat id for messages sent on
    /// behalf of a chat, such as channel posts and anonymous admins.
    pub sender_id: i64,
    pub sender_name: String,
    pub link: MediaLink,
    pub cache_hit: bool,
    pub shared_at: DateTime<Utc>,
}

impl MediaPost {
    /// Describe the delivery of `link`, shared in `msg`.
    ///
    /// # Errors
    ///
    /// Returns an error when the sender's user id does not fit in SQLite.
    pub fn new(msg: &Message, link: &MediaLink, cache_hit: bool) -> Result<Self> {
        // Messages sent on behalf of a chat carry a placeholder `from` user.
        let (sender_id, sender_name) = match (&msg.sender_chat, &msg.from) {
            (Some(chat), _) => (chat.id.0, chat_name(chat)),
            (None, Some(user)) => (
                i64::try_from(user.id.0)
                    .map_err(|_| MediaPostsError::UserIdOutOfRange(user.id.0))?,
                user.full_name(),
            ),
            (None, None) => (msg.chat.id.0, chat_name(&msg.chat)),
        };
        Ok(Self {
            chat_id: msg.chat.id,
            message_id: msg.id,
            sender_id,
            sender_name,
            link: link.clone(),
            cache_hit,
            shared_at: msg.date,
        })
    }
}

/// Per-chat log of delivered media links.
#[derive(Debug, Clone)]
pub struct MediaPosts {
    pool: SqlitePool,
}

impl MediaPosts {
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store `post`.
    ///
    /// # Errors
    ///
    /// Returns an error when the insert fails.
    pub async fn record(&self, post: &MediaPost) -> Result<()> {
        let chat_id = post.chat_id.0;
        let message_id = i64::from(post.message_id.0);
        let platform = post.link.platform.to_string();
        let created_at = db_timestamp(post.shared_at);
        sqlx::query!(
            r#"
            INSERT INTO
                media_posts (
                    chat_id,
                    message_id,
                    sender_id,
                    sender_name,
                    platform,
                    cache_key,
                    url,
                    cache_hit,
                    created_at
                )
            VALUES
                (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            chat_id,
            message_id,
            post.sender_id,
            post.sender_name,
            platform,
            post.link.cache_key,
            post.link.original_url,
            post.cache_hit,
            created_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Store the delivery of `link`, shared in `msg`.
    ///
    /// # Errors
    ///
    /// Returns an error when the sender id does not fit in SQLite or the insert fails.
    pub async fn record_delivery(
        &self,
        msg: &Message,
        link: &MediaLink,
        cache_hit: bool,
    ) -> Result<()> {
        self.record(&MediaPost::new(msg, link, cache_hit)?).await
    }

    /// Aggregate the posts of `chat_id` shared within `period` before `now`.
    ///
    /// # Errors
    ///
    /// Returns an error when a query fails.
    pub async fn stats(
        &self,
        chat_id: ChatId,
        period: StatsPeriod,
        now: DateTime<Utc>,
    ) -> Result<LinkStats> {
        let since = period.since(now).map(db_timestamp);
        let mut transaction = self.pool.begin().await?;
        let senders = top_senders(&mut transaction, chat_id, since.as_deref()).await?;
        let platforms = platform_counts(&mut transaction, chat_id, since.as_deref()).await?;
        let links = top_links(&mut transaction, chat_id, since.as_deref()).await?;
        transaction.commit().await?;
        Ok(LinkStats {
            period,
            senders,
            platforms,
            links,
        })
    }
}

// SQLite takes bare columns from the row that matched `MAX(id)`, so sender
// names and link URLs come from each group's latest post.

async fn top_senders(
    connection: &mut SqliteConnection,
    chat_id: ChatId,
    since: Option<&str>,
) -> Result<Vec<ShareCount>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            sender_name AS "sender_name!",
            COUNT(*) AS "count!: i64",
            MAX(id) AS "latest_id!: i64"
        FROM
            media_posts
        WHERE
            chat_id = ?1
            AND (?2 IS NULL OR created_at >= ?2)
        GROUP BY
            sender_id
        ORDER BY
            COUNT(*) DESC,
            MAX(id) DESC
        LIMIT
            ?3
        "#,
        chat_id.0,
        since,
        TOP_SENDERS,
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ShareCount {
            label: row.sender_name,
            count: row.count,
        })
        .collect())
}

async fn platform_counts(
    connection: &mut SqliteConnection,
    chat_id: ChatId,
    since: Option<&str>,
) -> Result<Vec<ShareCount>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            platform,
            COUNT(*) AS "count!: i64"
        FROM
            media_posts
        WHERE
            chat_id = ?1
            AND (?2 IS NULL OR created_at >= ?2)
        GROUP BY
            platform
        ORDER BY
            COUNT(*) DESC,
            platform
        "#,
        chat_id.0,
        since,
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ShareCount {
            label: row.platform,
            count: row.count,
        })
        .collect())
}

/// Links shared more than once, grouped by cache key so URL variants of the
/// same media count together.
async fn top_links(
    connection: &mut SqliteConnection,
    chat_id: ChatId,
    since: Option<&str>,
) -> Result<Vec<ShareCount>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            url AS "url!",
            COUNT(*) AS "count!: i64",
            MAX(id) AS "latest_id!: i64"
        FROM
            media_posts
        WHERE
            chat_id = ?1
            AND (?2 IS NULL OR created_at >= ?2)
        GROUP BY
            cache_key
        HAVING
            COUNT(*) > 1
        ORDER BY
            COUNT(*) DESC,
            MAX(id) DESC
        LIMIT
            ?3
        "#,
        chat_id.0,
        since,
        TOP_LINKS,
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ShareCount {
            label: row.url,
            count: row.count,
        })
        .collect())
}

fn chat_name(chat: &Chat) -> String {
    chat.title()
        .or_else(|| chat.username())
        .unwrap_or("unknown")
        .to_owned()
}

/// Match SQLite's `CURRENT_TIMESTAMP` format so text comparisons order by time.
fn db_timestamp(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Platform, db};
    use chrono::TimeZone;
    use claims::{assert_ok, assert_ok_eq, assert_some};
    use serde_json::json;

    const CHAT: ChatId = ChatId(-100);
    const OTHER_CHAT: ChatId = ChatId(-200);

    async fn posts() -> MediaPosts {
        MediaPosts::new(assert_ok!(db::connect("sqlite::memory:").await))
    }

    fn at(day: u32) -> DateTime<Utc> {
        assert_some!(Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).single())
    }

    fn link(platform: Platform, cache_key: &str, url: &str) -> MediaLink {
        MediaLink {
            platform,
            original_url: url.to_owned(),
            cache_key: cache_key.to_owned(),
            source_position: 0,
        }
    }

    fn post(chat_id: ChatId, sender: (i64, &str), link: MediaLink, day: u32) -> MediaPost {
        MediaPost {
            chat_id,
            message_id: MessageId(1),
            sender_id: sender.0,
            sender_name: sender.1.to_owned(),
            link,
            cache_hit: false,
            shared_at: at(day),
        }
    }

    fn count(label: &str, count: i64) -> ShareCount {
        ShareCount {
            label: label.to_owned(),
            count,
        }
    }

    fn reel(url: &str) -> MediaLink {
        link(Platform::Instagram, "instagram:post:A", url)
    }

    fn tiktok() -> MediaLink {
        link(
            Platform::Tiktok,
            "tiktok:video:1",
            "https://www.tiktok.com/@a/video/1",
        )
    }

    #[tokio::test]
    async fn records_the_delivery() {
        let posts = posts().await;
        let mut post = post(CHAT, (7, "Alice"), tiktok(), 13);
        post.message_id = MessageId(42);
        post.cache_hit = true;
        assert_ok!(posts.record(&post).await);

        let row = assert_ok!(
            sqlx::query!(
                r#"
                SELECT
                    chat_id,
                    message_id,
                    sender_id,
                    sender_name,
                    platform,
                    cache_key,
                    url,
                    cache_hit AS "cache_hit: bool",
                    created_at
                FROM
                    media_posts
                "#
            )
            .fetch_one(&posts.pool)
            .await
        );
        assert_eq!(row.chat_id, CHAT.0);
        assert_eq!(row.message_id, 42);
        assert_eq!(row.sender_id, 7);
        assert_eq!(row.sender_name, "Alice");
        assert_eq!(row.platform, "tiktok");
        assert_eq!(row.cache_key, "tiktok:video:1");
        assert_eq!(row.url, "https://www.tiktok.com/@a/video/1");
        assert!(row.cache_hit);
        assert_eq!(row.created_at, "2026-11-13 12:00:00");
    }

    #[tokio::test]
    async fn aggregates_only_the_requested_chat() {
        let posts = posts().await;
        for post in [
            post(CHAT, (1, "Alice"), reel("https://instagram.com/reel/A"), 10),
            post(CHAT, (1, "Alice"), tiktok(), 11),
            post(CHAT, (2, "Bob"), reel("https://instagram.com/p/A/"), 12),
            post(OTHER_CHAT, (2, "Bob"), tiktok(), 12),
            post(OTHER_CHAT, (2, "Bob"), tiktok(), 12),
        ] {
            assert_ok!(posts.record(&post).await);
        }

        assert_ok_eq!(
            posts.stats(CHAT, StatsPeriod::All, at(13)).await,
            LinkStats {
                period: StatsPeriod::All,
                senders: vec![count("Alice", 2), count("Bob", 1)],
                platforms: vec![count("instagram", 2), count("tiktok", 1)],
                links: vec![count("https://instagram.com/p/A/", 2)],
            }
        );
    }

    #[tokio::test]
    async fn groups_senders_by_id_under_their_latest_name() {
        let posts = posts().await;
        assert_ok!(posts.record(&post(CHAT, (1, "Al"), tiktok(), 10)).await);
        assert_ok!(posts.record(&post(CHAT, (1, "Alice"), tiktok(), 11)).await);
        assert_ok!(posts.record(&post(CHAT, (2, "Alice"), tiktok(), 12)).await);

        let stats = assert_ok!(posts.stats(CHAT, StatsPeriod::All, at(13)).await);
        assert_eq!(stats.senders, [count("Alice", 2), count("Alice", 1)]);
    }

    #[tokio::test]
    async fn period_excludes_older_posts() {
        let posts = posts().await;
        assert_ok!(posts.record(&post(CHAT, (1, "Alice"), tiktok(), 1)).await);
        assert_ok!(posts.record(&post(CHAT, (2, "Bob"), tiktok(), 6)).await);
        assert_ok!(posts.record(&post(CHAT, (2, "Bob"), tiktok(), 13)).await);

        let stats = assert_ok!(posts.stats(CHAT, StatsPeriod::Week, at(13)).await);
        assert_eq!(stats.senders, [count("Bob", 2)]);
        assert_eq!(stats.total(), 2);
    }

    fn message(sender_chat: Option<&serde_json::Value>) -> Message {
        assert_ok!(serde_json::from_value(json!({
            "message_id": 5,
            "date": 1_763_035_200,
            "chat": { "id": CHAT.0, "type": "supergroup", "title": "Group" },
            "from": { "id": 7, "is_bot": false, "first_name": "Alice", "last_name": "Smith" },
            "sender_chat": sender_chat,
            "text": "https://www.tiktok.com/@a/video/1",
        })))
    }

    #[test]
    fn sender_is_the_user_with_their_full_name() {
        let post = assert_ok!(MediaPost::new(&message(None), &tiktok(), false));
        assert_eq!(
            (post.sender_id, post.sender_name.as_str()),
            (7, "Alice Smith")
        );
        assert_eq!((post.chat_id, post.message_id), (CHAT, MessageId(5)));
    }

    #[test]
    fn sender_chat_takes_precedence_over_the_placeholder_user() {
        let sender_chat = json!({ "id": -300, "type": "channel", "title": "News" });
        let post = assert_ok!(MediaPost::new(
            &message(Some(&sender_chat)),
            &tiktok(),
            true
        ));
        assert_eq!((post.sender_id, post.sender_name.as_str()), (-300, "News"));
    }
}
