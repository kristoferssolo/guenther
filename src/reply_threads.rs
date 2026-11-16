//! Memory of in-character reply threads.
//!
//! Telegram only attaches the immediate parent to a reply, so every exchange
//! is stored with a link to its parent. A reply to any of Guenther's answers
//! then recovers the conversation that led to it.

use crate::{
    cache::CachedMedia,
    db,
    llm::{Speaker, ThreadMessage},
    utils::MediaKind,
};
use chrono::{DateTime, TimeDelta, Utc};
use sqlx::{SqliteConnection, SqlitePool};
use teloxide::types::{ChatId, MessageId};
use thiserror::Error;

/// Most messages of a thread sent to the model, counting back from the one
/// being replied to.
const MAX_THREAD_MESSAGES: i64 = 12;
/// How long stored messages are kept.
const RETENTION: TimeDelta = TimeDelta::days(14);

#[derive(Debug, Error)]
pub enum ReplyThreadsError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database contains invalid media kind `{0}`")]
    InvalidMediaKind(String),
}

pub type Result<T> = std::result::Result<T, ReplyThreadsError>;

/// One of Guenther's messages that a chat member replied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotMessage {
    pub message_id: MessageId,
    /// Text or caption; `None` for media sent without one.
    pub text: Option<String>,
    /// Photo or video of the message, fetched back by `file_id`.
    pub media: Option<CachedMedia>,
}

/// The conversation that led to one of Guenther's messages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Thread {
    /// Oldest first, ending with Guenther's message.
    pub messages: Vec<ThreadMessage>,
    /// Media Guenther posted when the thread started.
    pub media: Option<CachedMedia>,
}

impl Thread {
    /// A thread made of `parent` alone, for messages sent before the bot
    /// remembered threads or outside of one, such as media captions.
    pub fn opening(parent: &BotMessage) -> Self {
        Self {
            messages: vec![ThreadMessage {
                speaker: Speaker::Guenther,
                text: parent.text.clone(),
            }],
            media: parent.media.clone(),
        }
    }
}

/// A chat member's message to Guenther and his answer.
#[derive(Debug, Clone, Copy)]
pub struct Exchange<'a> {
    pub chat_id: ChatId,
    /// Guenther's message that was replied to; `None` for a mention.
    pub parent: Option<&'a BotMessage>,
    pub member_message_id: MessageId,
    pub member_name: &'a str,
    pub member_text: &'a str,
    pub answer_message_id: MessageId,
    pub answer_text: &'a str,
    /// Media of the thread, carried over to the answer.
    pub media: Option<&'a CachedMedia>,
    pub at: DateTime<Utc>,
}

/// Persistent store of reply threads, one row per message.
#[derive(Debug, Clone)]
pub struct ReplyThreads {
    pool: SqlitePool,
}

impl ReplyThreads {
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The thread that `parent` belongs to, or [`Thread::opening`] when it
    /// is not stored.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails or a stored row has an invalid
    /// media kind.
    pub async fn thread(&self, chat_id: ChatId, parent: &BotMessage) -> Result<Thread> {
        let thread = self.load(chat_id, parent.message_id).await?;
        if thread.messages.is_empty() {
            return Ok(Thread::opening(parent));
        }
        Ok(thread)
    }

    /// The stored messages ending with `message_id`, oldest first, at most
    /// `MAX_THREAD_MESSAGES`. The thread's media is the one stored with
    /// `message_id`.
    async fn load(&self, chat_id: ChatId, message_id: MessageId) -> Result<Thread> {
        let message_id = i64::from(message_id.0);
        let rows = sqlx::query!(
            r#"
            WITH RECURSIVE
                thread (
                    message_id,
                    parent_message_id,
                    member_name,
                    text,
                    media_kind,
                    media_file_id,
                    depth
                ) AS (
                    SELECT
                        message_id,
                        parent_message_id,
                        member_name,
                        text,
                        media_kind,
                        media_file_id,
                        1
                    FROM
                        reply_messages
                    WHERE
                        chat_id = ?1
                        AND message_id = ?2
                    UNION ALL
                    SELECT
                        parent.message_id,
                        parent.parent_message_id,
                        parent.member_name,
                        parent.text,
                        parent.media_kind,
                        parent.media_file_id,
                        thread.depth + 1
                    FROM
                        reply_messages AS parent
                        INNER JOIN thread ON parent.message_id = thread.parent_message_id
                    WHERE
                        parent.chat_id = ?1
                        AND thread.depth < ?3
                )
            SELECT
                member_name AS "member_name?: String",
                text AS "text?: String",
                media_kind AS "media_kind?: String",
                media_file_id AS "media_file_id?: String"
            FROM
                thread
            ORDER BY
                depth DESC
            "#,
            chat_id.0,
            message_id,
            MAX_THREAD_MESSAGES,
        )
        .fetch_all(&self.pool)
        .await?;

        let media = match rows.last() {
            Some(row) => cached_media(row.media_kind.as_deref(), row.media_file_id.as_deref())?,
            None => None,
        };
        let messages = rows
            .into_iter()
            .map(|row| ThreadMessage {
                speaker: row.member_name.map_or(Speaker::Guenther, Speaker::Member),
                text: row.text,
            })
            .collect();
        Ok(Thread { messages, media })
    }

    /// Store `exchange`, and its parent when it starts a thread, then drop
    /// messages older than `RETENTION`.
    ///
    /// # Errors
    ///
    /// Returns an error when a statement fails.
    pub async fn record(&self, exchange: &Exchange<'_>) -> Result<()> {
        let chat_id = exchange.chat_id.0;
        let created_at = db::timestamp(exchange.at);
        let parent_id = exchange.parent.map(|parent| i64::from(parent.message_id.0));
        let member_id = i64::from(exchange.member_message_id.0);
        let mut transaction = self.pool.begin().await?;

        if let Some(parent) = exchange.parent {
            insert(
                &mut transaction,
                Row {
                    chat_id,
                    message_id: i64::from(parent.message_id.0),
                    parent_message_id: None,
                    member_name: None,
                    text: parent.text.as_deref(),
                    media: parent.media.as_ref(),
                    created_at: &created_at,
                },
            )
            .await?;
        }
        insert(
            &mut transaction,
            Row {
                chat_id,
                message_id: member_id,
                parent_message_id: parent_id,
                member_name: Some(exchange.member_name),
                text: Some(exchange.member_text),
                media: None,
                created_at: &created_at,
            },
        )
        .await?;
        insert(
            &mut transaction,
            Row {
                chat_id,
                message_id: i64::from(exchange.answer_message_id.0),
                parent_message_id: Some(member_id),
                member_name: None,
                text: Some(exchange.answer_text),
                media: exchange.media,
                created_at: &created_at,
            },
        )
        .await?;

        let cutoff = exchange.at.checked_sub_signed(RETENTION).map(db::timestamp);
        sqlx::query!(
            r#"
            DELETE FROM
                reply_messages
            WHERE
                created_at < ?
            "#,
            cutoff,
        )
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct Row<'a> {
    chat_id: i64,
    message_id: i64,
    parent_message_id: Option<i64>,
    member_name: Option<&'a str>,
    text: Option<&'a str>,
    media: Option<&'a CachedMedia>,
    created_at: &'a str,
}

/// Insert `row` unless its message is already stored, which happens when a
/// second member replies to the same message of Guenther's.
async fn insert(connection: &mut SqliteConnection, row: Row<'_>) -> Result<()> {
    let media_kind = row.media.map(|media| <&str>::from(media.kind));
    let media_file_id = row.media.map(|media| media.file_id.as_str());
    sqlx::query!(
        r#"
        INSERT OR IGNORE INTO
            reply_messages (
                chat_id,
                message_id,
                parent_message_id,
                member_name,
                text,
                media_kind,
                media_file_id,
                created_at
            )
        VALUES
            (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        row.chat_id,
        row.message_id,
        row.parent_message_id,
        row.member_name,
        row.text,
        media_kind,
        media_file_id,
        row.created_at,
    )
    .execute(connection)
    .await?;
    Ok(())
}

fn cached_media(kind: Option<&str>, file_id: Option<&str>) -> Result<Option<CachedMedia>> {
    let (Some(kind), Some(file_id)) = (kind, file_id) else {
        return Ok(None);
    };
    let kind = kind
        .parse::<MediaKind>()
        .map_err(|_| ReplyThreadsError::InvalidMediaKind(kind.to_owned()))?;
    Ok(Some(CachedMedia {
        kind,
        file_id: file_id.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use claims::{assert_ok, assert_ok_eq, assert_some};

    const CHAT: ChatId = ChatId(-100);

    async fn threads() -> ReplyThreads {
        ReplyThreads::new(assert_ok!(db::connect("sqlite::memory:").await))
    }

    fn at(day: u32) -> DateTime<Utc> {
        assert_some!(Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).single())
    }

    fn video() -> CachedMedia {
        CachedMedia {
            kind: MediaKind::Video,
            file_id: "clip".to_owned(),
        }
    }

    fn bot_message(id: i32, text: &str, media: Option<CachedMedia>) -> BotMessage {
        BotMessage {
            message_id: MessageId(id),
            text: Some(text.to_owned()),
            media,
        }
    }

    fn guenther(text: &str) -> ThreadMessage {
        ThreadMessage {
            speaker: Speaker::Guenther,
            text: Some(text.to_owned()),
        }
    }

    fn member(name: &str, text: &str) -> ThreadMessage {
        ThreadMessage {
            speaker: Speaker::Member(name.to_owned()),
            text: Some(text.to_owned()),
        }
    }

    /// Record `name` replying `text` to `parent`, answered by message
    /// `answer_id` saying `answer`.
    async fn exchange(
        threads: &ReplyThreads,
        parent: Option<&BotMessage>,
        (member_id, name, text): (i32, &str, &str),
        (answer_id, answer): (i32, &str),
        day: u32,
    ) -> BotMessage {
        let thread = match parent {
            Some(parent) => assert_ok!(threads.thread(CHAT, parent).await),
            None => Thread::default(),
        };
        assert_ok!(
            threads
                .record(&Exchange {
                    chat_id: CHAT,
                    parent,
                    member_message_id: MessageId(member_id),
                    member_name: name,
                    member_text: text,
                    answer_message_id: MessageId(answer_id),
                    answer_text: answer,
                    media: thread.media.as_ref(),
                    at: at(day),
                })
                .await
        );
        bot_message(answer_id, answer, None)
    }

    #[tokio::test]
    async fn unknown_parent_opens_a_thread() {
        let threads = threads().await;
        let parent = bot_message(1, "Box. Now.", Some(video()));

        assert_ok_eq!(
            threads.thread(CHAT, &parent).await,
            Thread {
                messages: vec![guenther("Box. Now.")],
                media: Some(video()),
            }
        );
    }

    #[tokio::test]
    async fn follows_the_thread_back_to_its_opening_media() {
        let threads = threads().await;
        let caption = bot_message(1, "Box. Now.", Some(video()));
        let answer = exchange(
            &threads,
            Some(&caption),
            (2, "Kristofers", "why?"),
            (3, "Tyres are foked."),
            10,
        )
        .await;
        let answer = exchange(
            &threads,
            Some(&answer),
            (4, "Kristofers", "they look fine"),
            (5, "You look fine."),
            10,
        )
        .await;

        assert_ok_eq!(
            threads.thread(CHAT, &answer).await,
            Thread {
                messages: vec![
                    guenther("Box. Now."),
                    member("Kristofers", "why?"),
                    guenther("Tyres are foked."),
                    member("Kristofers", "they look fine"),
                    guenther("You look fine."),
                ],
                media: Some(video()),
            }
        );
    }

    #[tokio::test]
    async fn mention_starts_a_thread_with_the_member() {
        let threads = threads().await;
        let answer = exchange(&threads, None, (1, "Kristofers", "u here?"), (2, "No."), 10).await;

        assert_ok_eq!(
            threads.thread(CHAT, &answer).await,
            Thread {
                messages: vec![member("Kristofers", "u here?"), guenther("No.")],
                media: None,
            }
        );
    }

    #[tokio::test]
    async fn branches_keep_their_own_history() {
        let threads = threads().await;
        let caption = bot_message(1, "Box. Now.", None);
        exchange(
            &threads,
            Some(&caption),
            (2, "Anna", "why?"),
            (3, "Tyres."),
            10,
        )
        .await;
        let other = exchange(
            &threads,
            Some(&caption),
            (4, "Ben", "lol"),
            (5, "Fok off."),
            10,
        )
        .await;

        assert_ok_eq!(
            threads.thread(CHAT, &other).await,
            Thread {
                messages: vec![
                    guenther("Box. Now."),
                    member("Ben", "lol"),
                    guenther("Fok off."),
                ],
                media: None,
            }
        );
    }

    #[tokio::test]
    async fn keeps_only_the_latest_messages() {
        let threads = threads().await;
        let mut parent = bot_message(1, "0", None);
        let ids = (2..).step_by(2).zip((3..).step_by(2));
        for (turn, (member_id, answer_id)) in (1..=10).zip(ids) {
            parent = exchange(
                &threads,
                Some(&parent),
                (member_id, "Kristofers", "again"),
                (answer_id, &format!("{turn}")),
                10,
            )
            .await;
        }

        let thread = assert_ok!(threads.thread(CHAT, &parent).await);
        assert_ok_eq!(i64::try_from(thread.messages.len()), MAX_THREAD_MESSAGES);
        assert_eq!(thread.messages.last(), Some(&guenther("10")));
    }

    #[tokio::test]
    async fn prunes_expired_messages() {
        let threads = threads().await;
        let old = exchange(&threads, None, (1, "Kristofers", "u here?"), (2, "No."), 1).await;
        exchange(
            &threads,
            None,
            (3, "Kristofers", "now?"),
            (4, "Still no."),
            16,
        )
        .await;

        assert_ok_eq!(threads.thread(CHAT, &old).await, Thread::opening(&old));
    }

    #[tokio::test]
    async fn threads_are_kept_per_chat() {
        let threads = threads().await;
        let answer = exchange(&threads, None, (1, "Kristofers", "u here?"), (2, "No."), 10).await;

        assert_ok_eq!(
            threads.thread(ChatId(-200), &answer).await,
            Thread::opening(&answer)
        );
    }
}
