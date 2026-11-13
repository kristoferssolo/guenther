use crate::{
    cache::CachedMedia,
    handler::{caption::fetch_cached, delivery::extract_cached_media},
    llm::{Addressed, Captioner, LlmError, ReplyRequest},
    telegram_file::TelegramFiles,
    telemetry::ErrorChain,
    utils::MediaKind,
};
use std::{
    collections::{HashMap, hash_map::Entry},
    io,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use teloxide::{
    Bot, RequestError,
    prelude::*,
    types::{ChatId, MessageEntityKind, MessageId, ReplyParameters, UserId},
};
use tempfile::tempdir;
use thiserror::Error;
use tracing::{Instrument, debug, info, warn};

#[derive(Debug, Error)]
enum ReplyError {
    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] RequestError),
}

/// A person's text message addressed to the bot, by replying to one of its
/// messages or by mentioning its `@username`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotReply {
    chat_id: ChatId,
    message_id: MessageId,
    replier_name: String,
    reply_text: String,
    addressed: Addressed,
    /// Photo or video of the bot's message, fetched back by `file_id`.
    bot_media: Option<CachedMedia>,
}

impl BotReply {
    /// Detect a non-bot user's text reply to a message sent by `bot_id`, or
    /// a text message that mentions `@bot_name`. A reply to the bot wins
    /// over a mention in the same message.
    ///
    /// Only the immediate parent is available, since Telegram does not nest
    /// `reply_to_message`.
    pub fn detect(msg: &Message, bot_id: UserId, bot_name: &str) -> Option<Self> {
        let sender = msg.from.as_ref().filter(|user| !user.is_bot)?;
        let reply_text = msg.text().filter(|text| !text.trim().is_empty())?;
        // Telegram sets a forum topic's creation message as the parent of
        // every plain message in that topic, which is not a reply.
        let bot_parent = msg
            .reply_to_message()
            .filter(|parent| parent.forum_topic_created().is_none())
            .filter(|parent| parent.from.as_ref().is_some_and(|user| user.id == bot_id));
        let (addressed, bot_media) = match bot_parent {
            Some(parent) => (
                Addressed::Reply {
                    bot_message: parent
                        .text()
                        .or_else(|| parent.caption())
                        .map(str::to_owned),
                },
                extract_cached_media(parent, MediaKind::Video)
                    .or_else(|| extract_cached_media(parent, MediaKind::Image)),
            ),
            None if mentions(msg, bot_name) => (Addressed::Mention, None),
            None => return None,
        };
        Some(Self {
            chat_id: msg.chat.id,
            message_id: msg.id,
            replier_name: sender.first_name.clone(),
            reply_text: reply_text.to_owned(),
            addressed,
            bot_media,
        })
    }
}

/// Whether the text of `msg` mentions `@bot_name`. Telegram usernames are
/// case-insensitive.
fn mentions(msg: &Message, bot_name: &str) -> bool {
    msg.parse_entities().into_iter().flatten().any(|entity| {
        matches!(entity.kind(), MessageEntityKind::Mention)
            && entity
                .text()
                .strip_prefix('@')
                .is_some_and(|name| name.eq_ignore_ascii_case(bot_name))
    })
}

/// Answers replies to and mentions of the bot in character, at most once per
/// cooldown in each chat.
#[derive(Debug, Clone)]
pub struct ReplyResponder {
    captioner: Captioner,
    files: TelegramFiles,
    bot_id: UserId,
    bot_name: Arc<str>,
    cooldown: ChatCooldown,
}

impl ReplyResponder {
    pub fn new(
        captioner: Captioner,
        files: TelegramFiles,
        bot_id: UserId,
        bot_name: Arc<str>,
        cooldown: Duration,
    ) -> Self {
        Self {
            captioner,
            files,
            bot_id,
            bot_name,
            cooldown: ChatCooldown::new(cooldown),
        }
    }

    /// The message in `msg` when it replies to one of this bot's messages or
    /// mentions the bot.
    pub fn detect(&self, msg: &Message) -> Option<BotReply> {
        BotReply::detect(msg, self.bot_id, &self.bot_name)
    }

    /// Answer `reply` in the background. Replies during the chat's cooldown
    /// are dropped, and failures are only logged.
    pub fn answer(&self, bot: &Bot, reply: BotReply) {
        if !self.cooldown.try_start(reply.chat_id, Instant::now()) {
            debug!("Skipping in-character reply; the chat is cooling down");
            return;
        }
        let captioner = self.captioner.clone();
        let files = self.files.clone();
        let bot = bot.clone();
        tokio::spawn(
            async move {
                let started_at = Instant::now();
                match send_answer(&bot, &captioner, &files, reply).await {
                    Ok(()) => info!(elapsed = ?started_at.elapsed(), "Sent in-character reply"),
                    Err(err) => warn!(
                        error = %ErrorChain(&err),
                        elapsed = ?started_at.elapsed(),
                        "In-character reply failed"
                    ),
                }
            }
            .in_current_span(),
        );
    }
}

async fn send_answer(
    bot: &Bot,
    captioner: &Captioner,
    files: &TelegramFiles,
    reply: BotReply,
) -> Result<(), ReplyError> {
    let fetch_dir = tempdir()?;
    let request = ReplyRequest {
        addressed: reply.addressed,
        media: fetch_cached(bot, files, reply.bot_media, fetch_dir.path()).await,
        replier_name: Some(reply.replier_name),
        reply_text: reply.reply_text,
    };
    let line = captioner.reply(&request).await?;
    bot.send_message(reply.chat_id, line)
        .reply_parameters(ReplyParameters::new(reply.message_id))
        .await?;
    Ok(())
}

/// Allows one start per chat in every `interval`.
#[derive(Debug, Clone)]
struct ChatCooldown {
    interval: Duration,
    last_started: Arc<Mutex<HashMap<ChatId, Instant>>>,
}

impl ChatCooldown {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_started: Arc::default(),
        }
    }

    /// Record a start in `chat_id` at `now`, or return `false` while the
    /// chat's previous start is less than `interval` ago.
    fn try_start(&self, chat_id: ChatId, now: Instant) -> bool {
        let mut last_started = self
            .last_started
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Keeps the map limited to chats that are still cooling down.
        last_started.retain(|_, started| now.saturating_duration_since(*started) < self.interval);
        match last_started.entry(chat_id) {
            Entry::Occupied(_) => false,
            Entry::Vacant(entry) => {
                entry.insert(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_some, assert_some_eq};
    use rstest::rstest;
    use serde_json::{Value, json};

    const BOT_ID: UserId = UserId(42);
    const BOT_NAME: &str = "guenther_steiner_bot";
    const CHAT_ID: ChatId = ChatId(-100_123);

    fn user(id: u64, is_bot: bool) -> Value {
        json!({ "id": id, "is_bot": is_bot, "first_name": "Kristofers" })
    }

    fn person() -> Value {
        user(7, false)
    }

    fn bot_message(content: &Value) -> Value {
        let mut message = json!({
            "message_id": 1,
            "date": 1_700_000_000,
            "chat": { "id": CHAT_ID.0, "type": "supergroup", "title": "Pit wall" },
            "from": user(BOT_ID.0, true),
        });
        extend(&mut message, content);
        message
    }

    fn message(from: &Value, content: &Value) -> Message {
        let mut message = json!({
            "message_id": 2,
            "date": 1_700_000_060,
            "chat": { "id": CHAT_ID.0, "type": "supergroup", "title": "Pit wall" },
            "from": from,
        });
        extend(&mut message, content);
        assert_ok!(serde_json::from_value(message))
    }

    fn extend(message: &mut Value, content: &Value) {
        let fields = assert_some!(message.as_object_mut());
        fields.extend(assert_some!(content.as_object()).clone());
    }

    fn expected_reply(bot_message: Option<&str>, bot_media: Option<CachedMedia>) -> BotReply {
        BotReply {
            chat_id: CHAT_ID,
            message_id: MessageId(2),
            replier_name: "Kristofers".to_owned(),
            reply_text: "why box now?".to_owned(),
            addressed: Addressed::Reply {
                bot_message: bot_message.map(str::to_owned),
            },
            bot_media,
        }
    }

    #[test]
    fn detects_text_reply_to_bot_text() {
        let msg = message(
            &person(),
            &json!({
                "text": "why box now?",
                "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
            }),
        );

        assert_some_eq!(
            BotReply::detect(&msg, BOT_ID, BOT_NAME),
            expected_reply(Some("Box. Now."), None)
        );
    }

    #[test]
    fn detects_reply_to_bot_text_inside_a_topic() {
        let msg = message(
            &person(),
            &json!({
                "message_thread_id": 5,
                "is_topic_message": true,
                "text": "why box now?",
                "reply_to_message": bot_message(&json!({
                    "message_thread_id": 5,
                    "is_topic_message": true,
                    "text": "Box. Now.",
                })),
            }),
        );

        assert_some_eq!(
            BotReply::detect(&msg, BOT_ID, BOT_NAME),
            expected_reply(Some("Box. Now."), None)
        );
    }

    #[test]
    fn detects_reply_to_bot_photo_with_largest_size() {
        let msg = message(
            &person(),
            &json!({
                "text": "why box now?",
                "reply_to_message": bot_message(&json!({
                    "caption": "We look like wankers.",
                    "photo": [
                        { "file_id": "small", "file_unique_id": "s", "file_size": 1, "width": 90, "height": 60 },
                        { "file_id": "large", "file_unique_id": "l", "file_size": 2, "width": 1280, "height": 853 },
                    ],
                })),
            }),
        );

        assert_some_eq!(
            BotReply::detect(&msg, BOT_ID, BOT_NAME),
            expected_reply(
                Some("We look like wankers."),
                Some(CachedMedia {
                    kind: MediaKind::Image,
                    file_id: "large".to_owned(),
                })
            )
        );
    }

    #[test]
    fn detects_reply_to_bot_video() {
        let msg = message(
            &person(),
            &json!({
                "text": "why box now?",
                "reply_to_message": bot_message(&json!({
                    "video": {
                        "file_id": "clip", "file_unique_id": "c", "file_size": 3,
                        "width": 720, "height": 1280, "duration": 12, "mime_type": "video/mp4",
                    },
                })),
            }),
        );

        assert_some_eq!(
            BotReply::detect(&msg, BOT_ID, BOT_NAME),
            expected_reply(
                None,
                Some(CachedMedia {
                    kind: MediaKind::Video,
                    file_id: "clip".to_owned(),
                })
            )
        );
    }

    #[rstest]
    #[case::standalone("u here @guenther_steiner_bot ?", 7, json!({}))]
    #[case::any_case("@Guenther_Steiner_Bot box?", 0, json!({}))]
    #[case::in_reply_to_another_user("@guenther_steiner_bot thoughts?", 0, json!({
        "reply_to_message": bot_message(&json!({ "from": user(8, false), "text": "Box. Now." })),
    }))]
    fn detects_mention_of_the_bot(#[case] text: &str, #[case] offset: usize, #[case] extra: Value) {
        let mut content = json!({
            "text": text,
            "entities": [{ "type": "mention", "offset": offset, "length": 21 }],
        });
        extend(&mut content, &extra);
        let msg = message(&person(), &content);

        let reply = assert_some!(BotReply::detect(&msg, BOT_ID, BOT_NAME));
        assert_eq!(reply.addressed, Addressed::Mention);
        assert_none!(reply.bot_media);
    }

    #[rstest]
    #[case::not_a_reply(person(), json!({ "text": "why box now?" }))]
    #[case::reply_from_a_bot(user(9, true), json!({
        "text": "why box now?",
        "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
    }))]
    #[case::reply_to_another_user(person(), json!({
        "text": "why box now?",
        "reply_to_message": bot_message(&json!({ "from": user(8, false), "text": "Box. Now." })),
    }))]
    #[case::reply_to_another_bot(person(), json!({
        "text": "why box now?",
        "reply_to_message": bot_message(&json!({ "from": user(9, true), "text": "Box. Now." })),
    }))]
    #[case::plain_message_in_a_bot_created_topic(person(), json!({
        "message_thread_id": 5,
        "is_topic_message": true,
        "text": "why box now?",
        "reply_to_message": bot_message(&json!({
            "message_id": 5,
            "message_thread_id": 5,
            "is_topic_message": true,
            "forum_topic_created": { "name": "Race day", "icon_color": 7_322_096 },
        })),
    }))]
    #[case::blank_text(person(), json!({
        "text": "   ",
        "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
    }))]
    #[case::mention_of_another_user(person(), json!({
        "text": "@someone_else box?",
        "entities": [{ "type": "mention", "offset": 0, "length": 13 }],
    }))]
    #[case::name_without_mention_entity(person(), json!({ "text": "@guenther_steiner_bot box?" }))]
    #[case::mention_from_a_bot(user(9, true), json!({
        "text": "@guenther_steiner_bot box?",
        "entities": [{ "type": "mention", "offset": 0, "length": 21 }],
    }))]
    #[case::media_without_text(person(), json!({
        "caption": "why box now?",
        "photo": [{ "file_id": "p", "file_unique_id": "p", "file_size": 1, "width": 90, "height": 60 }],
        "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
    }))]
    fn ignores_messages_not_addressed_to_the_bot(#[case] from: Value, #[case] content: Value) {
        assert_none!(BotReply::detect(
            &message(&from, &content),
            BOT_ID,
            BOT_NAME
        ));
    }

    #[test]
    fn cooldown_limits_each_chat_separately() {
        let cooldown = ChatCooldown::new(Duration::from_secs(30));
        let start = Instant::now();
        let during = assert_some!(start.checked_add(Duration::from_secs(29)));
        let after = assert_some!(start.checked_add(Duration::from_secs(30)));

        assert!(cooldown.try_start(CHAT_ID, start));
        assert!(!cooldown.try_start(CHAT_ID, start));
        assert!(!cooldown.try_start(CHAT_ID, during));
        assert!(cooldown.try_start(ChatId(1), during));
        assert!(cooldown.try_start(CHAT_ID, after));
        assert!(!cooldown.try_start(CHAT_ID, after));
    }

    #[test]
    fn zero_cooldown_never_limits() {
        let cooldown = ChatCooldown::new(Duration::ZERO);
        let now = Instant::now();

        assert!(cooldown.try_start(CHAT_ID, now));
        assert!(cooldown.try_start(CHAT_ID, now));
    }
}
