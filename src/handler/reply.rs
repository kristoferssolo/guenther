use crate::{
    cache::CachedMedia,
    handler::{caption::fetch_cached, delivery::extract_cached_media},
    llm::{Captioner, Conversation, LinkedPage, LlmError, ReplyRequest, Speaker, ThreadMessage},
    prompt_store::PromptStore,
    reply_threads::{BotMessage, Exchange, Parent, ReplyThreads, Thread},
    telegram_file::TelegramFiles,
    telemetry::ErrorChain,
    utils::{MediaKind, replied_to, text_or_caption},
    web_page::PageFetcher,
};
use chrono::Utc;
use std::{
    collections::{HashMap, hash_map::Entry},
    io,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use teloxide::{
    Bot, RequestError,
    prelude::*,
    types::{ChatId, MessageEntityKind, MessageId, ReplyParameters, User, UserId},
};
use tempfile::tempdir;
use thiserror::Error;
use tracing::{Instrument, debug, info, warn};
use url::Url;

#[derive(Debug, Error)]
enum ReplyError {
    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Telegram request failed: {0}")]
    Telegram(#[from] RequestError),
}

/// A person's message addressed to the bot, by replying to one of its
/// messages, by mentioning its `@username`, or with `/analysis`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotReply {
    chat_id: ChatId,
    /// The message the bot's answer replies to.
    message_id: MessageId,
    replier_id: UserId,
    replier_name: String,
    reply_text: String,
    target: ReplyTarget,
    /// Web page to read before answering.
    link: Option<Url>,
}

/// What a [`BotReply`] responds to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplyTarget {
    /// The bot's message that was replied to.
    Thread(BotMessage),
    Mention,
    Analysis {
        /// The message that `/analysis` replied to.
        subject: AnalysedMessage,
        /// The `/analysis` message itself.
        request_id: MessageId,
        /// The request as remembered in the thread, with its question.
        request_text: String,
    },
}

/// A chat message the bot is asked to analyse.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AnalysedMessage {
    message_id: MessageId,
    message: ThreadMessage,
    media: Option<CachedMedia>,
}

impl BotReply {
    /// Detect a non-bot user's text reply to a message sent by `bot_id`, or
    /// a text message that mentions `@bot_name`. A reply to the bot wins
    /// over a mention in the same message.
    ///
    /// Only the immediate parent is available, since Telegram does not nest
    /// `reply_to_message`; [`ReplyThreads`] recovers the rest of the thread.
    ///
    /// A mention in reply to someone else's message asks for an analysis of
    /// that message, like `/analysis`, but the answer replies to the mention.
    ///
    /// The first link of the message is read before answering. An analysis
    /// without one falls back to the analysed message, so `@bot thoughts?`
    /// on someone else's link reads that link.
    pub fn detect(msg: &Message, bot_id: UserId, bot_name: &str) -> Option<Self> {
        let sender = human_sender(msg)?;
        let reply_text = msg.text().filter(|text| !text.trim().is_empty())?;
        let replied_to = replied_to(msg);
        let bot_parent =
            replied_to.filter(|parent| parent.from.as_ref().is_some_and(|user| user.id == bot_id));
        let target = match (bot_parent, replied_to) {
            (Some(parent), _) => ReplyTarget::Thread(BotMessage {
                message_id: parent.id,
                text: text_or_caption(parent).map(str::to_owned),
                media: extract_cached_media(parent, MediaKind::Video)
                    .or_else(|| extract_cached_media(parent, MediaKind::Image)),
            }),
            (None, _) if !mentions(msg, bot_name) => return None,
            (None, Some(subject)) => ReplyTarget::Analysis {
                subject: AnalysedMessage::new(subject, bot_id),
                request_id: msg.id,
                request_text: reply_text.to_owned(),
            },
            (None, None) => ReplyTarget::Mention,
        };
        let link = first_link(msg).or_else(|| {
            replied_to
                .filter(|_| matches!(target, ReplyTarget::Analysis { .. }))
                .and_then(first_link)
        });
        Some(Self {
            chat_id: msg.chat.id,
            message_id: msg.id,
            replier_id: sender.id,
            replier_name: sender.first_name.clone(),
            reply_text: reply_text.to_owned(),
            target,
            link,
        })
    }

    /// Build the `/analysis` request in `msg` with `question`, or `None`
    /// when `msg` does not reply to a message or comes from a bot.
    ///
    /// The answer replies to the analysed message. The first link of `msg`,
    /// or else of the analysed message, is read before answering.
    pub fn analysis(msg: &Message, bot_id: UserId, question: &str) -> Option<Self> {
        let sender = human_sender(msg)?;
        let subject = replied_to(msg)?;
        Some(Self {
            chat_id: msg.chat.id,
            message_id: subject.id,
            replier_id: sender.id,
            replier_name: sender.first_name.clone(),
            reply_text: question.trim().to_owned(),
            target: ReplyTarget::Analysis {
                subject: AnalysedMessage::new(subject, bot_id),
                request_id: msg.id,
                request_text: format!("/analysis {}", question.trim())
                    .trim_end()
                    .to_owned(),
            },
            link: first_link(msg).or_else(|| first_link(subject)),
        })
    }

    /// The exchange to remember once `answer` was sent with `text`, so
    /// replies to the answer keep its context. An analysis is remembered as
    /// the analysed message, the `/analysis` request, and the answer.
    fn exchange<'a>(
        &'a self,
        answer: &'a Message,
        text: &'a str,
        media: Option<&'a CachedMedia>,
    ) -> Exchange<'a> {
        let (parent, member_message_id, member_text) = match &self.target {
            ReplyTarget::Thread(parent) => (
                Some(Parent::from(parent)),
                self.message_id,
                self.reply_text.as_str(),
            ),
            ReplyTarget::Mention => (None, self.message_id, self.reply_text.as_str()),
            ReplyTarget::Analysis {
                subject,
                request_id,
                request_text,
            } => (
                Some(Parent {
                    message_id: subject.message_id,
                    member_name: match &subject.message.speaker {
                        Speaker::Guenther => None,
                        Speaker::Member(name) => Some(name),
                    },
                    text: subject.message.text.as_deref(),
                    media: subject.media.as_ref(),
                }),
                *request_id,
                request_text.as_str(),
            ),
        };
        Exchange {
            chat_id: self.chat_id,
            parent,
            member_message_id,
            member_name: &self.replier_name,
            member_text,
            answer_message_id: answer.id,
            answer_text: text,
            media,
            at: Utc::now(),
        }
    }
}

impl AnalysedMessage {
    /// `msg` with its text and media, attributed to Guenther when `bot_id`
    /// sent it.
    fn new(msg: &Message, bot_id: UserId) -> Self {
        let speaker = match &msg.from {
            Some(user) if user.id == bot_id => Speaker::Guenther,
            Some(user) => Speaker::Member(user.first_name.clone()),
            None => Speaker::Member(
                msg.sender_chat
                    .as_ref()
                    .and_then(|chat| chat.title())
                    .unwrap_or("Someone")
                    .to_owned(),
            ),
        };
        Self {
            message_id: msg.id,
            message: ThreadMessage {
                speaker,
                text: text_or_caption(msg).map(str::to_owned),
            },
            media: analysed_media(msg),
        }
    }
}

/// The person who sent `msg`, or `None` for bots and channels.
fn human_sender(msg: &Message) -> Option<&User> {
    msg.from.as_ref().filter(|user| !user.is_bot)
}

/// The photo, video, or GIF of `msg`. Telegram sends GIFs as silent MP4s.
fn analysed_media(msg: &Message) -> Option<CachedMedia> {
    extract_cached_media(msg, MediaKind::Video)
        .or_else(|| {
            msg.animation().map(|animation| CachedMedia {
                kind: MediaKind::Video,
                file_id: animation.file.id.0.clone(),
            })
        })
        .or_else(|| extract_cached_media(msg, MediaKind::Image))
}

/// The first http(s) link in the text or caption of `msg`, including bare
/// domains Telegram recognizes and links hidden behind text.
fn first_link(msg: &Message) -> Option<Url> {
    msg.parse_entities()
        .or_else(|| msg.parse_caption_entities())
        .into_iter()
        .flatten()
        .find_map(|entity| {
            let url = match entity.kind() {
                MessageEntityKind::Url if entity.text().contains("://") => {
                    Url::parse(entity.text()).ok()?
                }
                MessageEntityKind::Url => Url::parse(&format!("https://{}", entity.text())).ok()?,
                MessageEntityKind::TextLink { url } => url.clone(),
                _ => return None,
            };
            matches!(url.scheme(), "http" | "https").then_some(url)
        })
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
/// cooldown in each chat, keeping the context of earlier replies.
#[derive(Debug, Clone)]
pub struct ReplyResponder {
    captioner: Captioner,
    files: TelegramFiles,
    threads: ReplyThreads,
    pages: PageFetcher,
    bot_id: UserId,
    bot_name: Arc<str>,
    cooldown: ChatCooldown,
}

impl ReplyResponder {
    pub fn new(
        captioner: Captioner,
        files: TelegramFiles,
        threads: ReplyThreads,
        pages: PageFetcher,
        bot_id: UserId,
        bot_name: Arc<str>,
        cooldown: Duration,
    ) -> Self {
        Self {
            captioner,
            files,
            threads,
            pages,
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

    /// The store of the system prompts, edited with `/prompt`.
    pub const fn prompts(&self) -> &PromptStore {
        self.captioner.prompts()
    }

    /// The `/analysis` request in `msg`; see [`BotReply::analysis`].
    pub fn analysis(&self, msg: &Message, question: &str) -> Option<BotReply> {
        BotReply::analysis(msg, self.bot_id, question)
    }

    /// Answer `reply` in the background. Replies and analyses during the
    /// chat's cooldown are dropped, and failures are only logged.
    pub fn answer(&self, bot: &Bot, reply: BotReply) {
        if !self.cooldown.try_start(reply.chat_id, Instant::now()) {
            debug!("Skipping in-character reply; the chat is cooling down");
            return;
        }
        let responder = self.clone();
        let bot = bot.clone();
        tokio::spawn(
            async move {
                let started_at = Instant::now();
                match responder.send_answer(&bot, reply).await {
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

    async fn send_answer(&self, bot: &Bot, reply: BotReply) -> Result<(), ReplyError> {
        let ((conversation, media), linked_page) =
            tokio::join!(self.conversation(&reply), self.linked_page(&reply));
        let fetch_dir = tempdir()?;
        let request = ReplyRequest {
            conversation,
            media: fetch_cached(bot, &self.files, media.clone(), fetch_dir.path()).await,
            replier_id: Some(reply.replier_id),
            replier_name: Some(reply.replier_name.clone()),
            reply_text: reply.reply_text.clone(),
            linked_page,
        };
        let line = self.captioner.reply(&request).await?;
        let answer = bot
            .send_message(reply.chat_id, &line)
            .reply_parameters(ReplyParameters::new(reply.message_id))
            .await?;

        let exchange = reply.exchange(&answer, &line, media.as_ref());
        if let Err(err) = self.threads.record(&exchange).await {
            warn!(error = %ErrorChain(&err), "Failed to remember the reply thread");
        }
        Ok(())
    }

    /// The page `reply` links, with what could be read from it.
    async fn linked_page(&self, reply: &BotReply) -> Option<LinkedPage> {
        let url = reply.link.clone()?;
        let page = self
            .pages
            .fetch(&url)
            .await
            .inspect_err(
                |err| info!(error = %ErrorChain(err), %url, "Could not read the linked page"),
            )
            .ok();
        Some(LinkedPage { url, page })
    }

    /// The conversation `reply` continues, with its media. A thread falls
    /// back to the replied-to message alone when the stored thread cannot be
    /// read.
    async fn conversation(&self, reply: &BotReply) -> (Conversation, Option<CachedMedia>) {
        match &reply.target {
            ReplyTarget::Thread(parent) => {
                let thread = self
                    .threads
                    .thread(reply.chat_id, parent, Utc::now())
                    .await
                    .unwrap_or_else(|err| {
                        warn!(error = %ErrorChain(&err), "Failed to load the reply thread");
                        Thread::opening(parent)
                    });
                (Conversation::Thread(thread.messages), thread.media)
            }
            ReplyTarget::Mention => (Conversation::Mention, None),
            ReplyTarget::Analysis { subject, .. } => (
                Conversation::Analysis(subject.message.clone()),
                subject.media.clone(),
            ),
        }
    }
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
            replier_id: UserId(7),
            replier_name: "Kristofers".to_owned(),
            reply_text: "why box now?".to_owned(),
            target: ReplyTarget::Thread(BotMessage {
                message_id: MessageId(1),
                text: bot_message.map(str::to_owned),
                media: bot_media,
            }),
            link: None,
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
    #[case::plain_message_in_a_topic("@guenther_steiner_bot box?", 0, json!({
        "message_thread_id": 5,
        "is_topic_message": true,
        "reply_to_message": bot_message(&json!({
            "message_id": 5,
            "message_thread_id": 5,
            "is_topic_message": true,
            "forum_topic_created": { "name": "Race day", "icon_color": 7_322_096 },
        })),
    }))]
    fn detects_mention_of_the_bot(#[case] text: &str, #[case] offset: usize, #[case] extra: Value) {
        let mut content = json!({
            "text": text,
            "entities": [{ "type": "mention", "offset": offset, "length": 21 }],
        });
        extend(&mut content, &extra);
        let msg = message(&person(), &content);

        let reply = assert_some!(BotReply::detect(&msg, BOT_ID, BOT_NAME));
        assert_eq!(reply.target, ReplyTarget::Mention);
    }

    #[test]
    fn mention_in_reply_to_a_members_photo_analyses_it() {
        let msg = message(
            &person(),
            &json!({
                "text": "@guenther_steiner_bot thoughts?",
                "entities": [{ "type": "mention", "offset": 0, "length": 21 }],
                "reply_to_message": bot_message(&json!({
                    "from": { "id": 8, "is_bot": false, "first_name": "Janis" },
                    "photo": [{ "file_id": "p", "file_unique_id": "p", "file_size": 1, "width": 90, "height": 60 }],
                })),
            }),
        );

        let reply = assert_some!(BotReply::detect(&msg, BOT_ID, BOT_NAME));

        assert_eq!(reply.message_id, MessageId(2));
        assert_eq!(
            reply.target,
            ReplyTarget::Analysis {
                subject: AnalysedMessage {
                    message_id: MessageId(1),
                    message: ThreadMessage {
                        speaker: Speaker::Member("Janis".to_owned()),
                        text: None,
                    },
                    media: Some(CachedMedia {
                        kind: MediaKind::Image,
                        file_id: "p".to_owned(),
                    }),
                },
                request_id: MessageId(2),
                request_text: "@guenther_steiner_bot thoughts?".to_owned(),
            }
        );
    }

    fn url_entity(offset: usize, length: usize) -> Value {
        json!({ "type": "url", "offset": offset, "length": length })
    }

    fn mention_entity() -> Value {
        json!({ "type": "mention", "offset": 0, "length": 21 })
    }

    #[rstest]
    #[case::url_in_reply(json!({
        "text": "seen https://example.com/haas ?",
        "entities": [url_entity(5, 24)],
        "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
    }), "https://example.com/haas")]
    #[case::bare_domain_in_mention(json!({
        "text": "@guenther_steiner_bot example.com/haas",
        "entities": [mention_entity(), url_entity(22, 16)],
    }), "https://example.com/haas")]
    #[case::hidden_text_link(json!({
        "text": "@guenther_steiner_bot read this",
        "entities": [
            mention_entity(),
            { "type": "text_link", "offset": 22, "length": 9, "url": "https://example.com/haas" },
        ],
    }), "https://example.com/haas")]
    #[case::mention_on_another_members_link(json!({
        "text": "@guenther_steiner_bot thoughts?",
        "entities": [mention_entity()],
        "reply_to_message": bot_message(&json!({
            "from": user(8, false),
            "caption": "look https://example.com/haas",
            "caption_entities": [url_entity(5, 24)],
            "photo": [{ "file_id": "p", "file_unique_id": "p", "file_size": 1, "width": 90, "height": 60 }],
        })),
    }), "https://example.com/haas")]
    #[case::own_link_wins(json!({
        "text": "@guenther_steiner_bot https://example.com/haas",
        "entities": [mention_entity(), url_entity(22, 24)],
        "reply_to_message": bot_message(&json!({
            "from": user(8, false),
            "text": "https://example.com/other",
            "entities": [url_entity(0, 25)],
        })),
    }), "https://example.com/haas")]
    fn detects_linked_page(#[case] content: Value, #[case] expected: &str) {
        let reply = assert_some!(BotReply::detect(
            &message(&person(), &content),
            BOT_ID,
            BOT_NAME
        ));

        assert_some_eq!(reply.link.as_ref().map(Url::as_str), expected);
    }

    #[rstest]
    #[case::no_link(json!({
        "text": "@guenther_steiner_bot box?",
        "entities": [mention_entity()],
    }))]
    #[case::non_web_text_link(json!({
        "text": "@guenther_steiner_bot ask Kristofers",
        "entities": [
            mention_entity(),
            { "type": "text_link", "offset": 26, "length": 10, "url": "tg://user?id=7" },
        ],
    }))]
    #[case::link_in_the_bots_message(json!({
        "text": "why?",
        "reply_to_message": bot_message(&json!({
            "text": "https://example.com/haas",
            "entities": [url_entity(0, 24)],
        })),
    }))]
    fn ignores_messages_without_a_web_link(#[case] content: Value) {
        let reply = assert_some!(BotReply::detect(
            &message(&person(), &content),
            BOT_ID,
            BOT_NAME
        ));

        assert_none!(reply.link);
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

    fn analysis_command(content: &Value) -> Message {
        let mut command = json!({
            "text": "/analysis",
            "entities": [{ "type": "bot_command", "offset": 0, "length": 9 }],
        });
        extend(&mut command, content);
        message(&person(), &command)
    }

    #[test]
    fn analysis_reads_another_members_gif() {
        let msg = analysis_command(&json!({
            "reply_to_message": bot_message(&json!({
                "from": user(8, false),
                "caption": "Kowalski, analysis",
                "animation": {
                    "file_id": "gif", "file_unique_id": "g", "file_size": 4,
                    "width": 480, "height": 270, "duration": 2, "mime_type": "video/mp4",
                },
                "document": { "file_id": "gif", "file_unique_id": "g", "file_size": 4 },
            })),
        }));

        let reply = assert_some!(BotReply::analysis(&msg, BOT_ID, "  is he right? "));

        assert_eq!(reply.message_id, MessageId(1));
        assert_eq!(reply.reply_text, "is he right?");
        assert_eq!(
            reply.target,
            ReplyTarget::Analysis {
                subject: AnalysedMessage {
                    message_id: MessageId(1),
                    message: ThreadMessage {
                        speaker: Speaker::Member("Kristofers".to_owned()),
                        text: Some("Kowalski, analysis".to_owned()),
                    },
                    media: Some(CachedMedia {
                        kind: MediaKind::Video,
                        file_id: "gif".to_owned(),
                    }),
                },
                request_id: MessageId(2),
                request_text: "/analysis is he right?".to_owned(),
            }
        );
    }

    #[rstest]
    #[case::own_link(json!({
        "text": "/analysis https://example.com/haas",
        "entities": [
            { "type": "bot_command", "offset": 0, "length": 9 },
            url_entity(10, 24),
        ],
        "reply_to_message": bot_message(&json!({
            "text": "https://example.com/other",
            "entities": [url_entity(0, 25)],
        })),
    }), "https://example.com/haas")]
    #[case::analysed_link(json!({
        "reply_to_message": bot_message(&json!({
            "text": "https://example.com/other",
            "entities": [url_entity(0, 25)],
        })),
    }), "https://example.com/other")]
    fn analysis_reads_the_first_link(#[case] content: Value, #[case] expected: &str) {
        let reply = assert_some!(BotReply::analysis(&analysis_command(&content), BOT_ID, ""));

        assert_some_eq!(reply.link.as_ref().map(Url::as_str), expected);
    }

    #[test]
    fn analysis_is_remembered_after_the_analysed_message() {
        let msg = analysis_command(&json!({
            "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
        }));
        let reply = assert_some!(BotReply::analysis(&msg, BOT_ID, ""));
        let answer = message(&user(BOT_ID.0, true), &json!({ "text": "Fok." }));

        let exchange = reply.exchange(&answer, "Fok.", None);
        let parent = assert_some!(exchange.parent);
        assert_eq!(parent.message_id, MessageId(1));
        assert_none!(parent.member_name);
        assert_some_eq!(parent.text, "Box. Now.");
        assert_eq!(exchange.member_message_id, MessageId(2));
        assert_eq!(exchange.member_text, "/analysis");
    }

    #[rstest]
    #[case::not_a_reply(person(), json!({}))]
    #[case::from_a_bot(user(9, true), json!({
        "reply_to_message": bot_message(&json!({ "text": "Box. Now." })),
    }))]
    #[case::plain_message_in_a_topic(person(), json!({
        "message_thread_id": 5,
        "is_topic_message": true,
        "reply_to_message": bot_message(&json!({
            "message_id": 5,
            "message_thread_id": 5,
            "is_topic_message": true,
            "forum_topic_created": { "name": "Race day", "icon_color": 7_322_096 },
        })),
    }))]
    fn analysis_needs_a_replied_to_message(#[case] from: Value, #[case] content: Value) {
        let mut command = json!({ "text": "/analysis" });
        extend(&mut command, &content);

        assert_none!(BotReply::analysis(&message(&from, &command), BOT_ID, ""));
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
