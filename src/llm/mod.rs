//! LLM-written media captions and in-character replies via `OpenRouter`.
//!
//! Media waits a bounded time for a [`Captioner`] result. A late caption
//! replaces the `comments.txt` caption the media went out with, and failures
//! keep that static caption, so captioning never blocks delivery for long.

mod error;
mod media;
mod openrouter;
mod prompt;

pub use crate::llm::{
    error::{LlmError, Result},
    media::LocalMedia,
};

use crate::{
    comments::Comments,
    config::{OpenRouterConfig, Platform},
    ffmpeg::Ffmpeg,
    llm::{
        media::{LabeledPart, MediaTools},
        openrouter::{ContentPart, OpenRouterClient},
        prompt::{
            PromptContext, REPLY_SYSTEM_PROMPT, ReplyContext, STYLE_EXAMPLE_COUNT, SYSTEM_PROMPT,
            sanitize_caption,
        },
    },
    utils::MediaKind,
    web_page::WebPage,
};
use std::{sync::Arc, time::Duration};
use teloxide::types::UserId;
use tempfile::tempdir;
use tracing::debug;
use url::Url;

/// Everything the model sees about one shared link.
#[derive(Debug, Clone)]
pub struct CaptionRequest {
    pub platform: Platform,
    pub sender_id: Option<UserId>,
    pub sender_name: Option<String>,
    pub message_text: Option<String>,
    pub source_text: Option<String>,
    pub media: Vec<LocalMedia>,
}

/// Who wrote a message in a reply thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    Guenther,
    /// A chat member, by first name.
    Member(String),
}

/// One earlier message of a reply thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMessage {
    pub speaker: Speaker,
    /// Text or caption; `None` for media sent without one.
    pub text: Option<String>,
}

/// A web page linked in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedPage {
    pub url: Url,
    /// `None` when the page could not be read.
    pub page: Option<WebPage>,
}

/// What a chat member's message to Guenther responds to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversation {
    /// Earlier messages of the thread, oldest first, ending with Guenther's
    /// message that was replied to.
    Thread(Vec<ThreadMessage>),
    /// A mention of Guenther outside of a thread.
    Mention,
    /// A chat message the member asked Guenther to analyse with `/analysis`.
    Analysis(ThreadMessage),
}

/// Everything the model sees about a chat member's reply to Guenther.
#[derive(Debug, Clone)]
pub struct ReplyRequest {
    pub conversation: Conversation,
    /// Media Guenther posted when the thread started, or the media of the
    /// analysed message.
    pub media: Vec<LocalMedia>,
    pub replier_id: Option<UserId>,
    pub replier_name: Option<String>,
    /// The member's message, or the question asked with `/analysis`.
    pub reply_text: String,
    /// The page the chat member linked, if any.
    pub linked_page: Option<LinkedPage>,
}

/// Writes captions in Guenther's voice from post context and media.
#[derive(Debug, Clone)]
pub struct Captioner {
    client: OpenRouterClient,
    tools: MediaTools,
    comments: Arc<Comments>,
    initial_wait: Duration,
    /// The bot owner, whom Guenther does not insult.
    owner: Option<UserId>,
}

impl Captioner {
    /// Build a captioner that takes its style examples from `comments` and
    /// goes easy on `owner`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(
        config: &OpenRouterConfig,
        ffmpeg: Ffmpeg,
        comments: Arc<Comments>,
        owner: Option<UserId>,
    ) -> Result<Self> {
        Ok(Self {
            client: OpenRouterClient::new(&config.api_key, &config.model)?,
            tools: MediaTools::new(ffmpeg, config.video_input),
            comments,
            initial_wait: config.caption_wait,
            owner,
        })
    }

    fn is_owner(&self, user: Option<UserId>) -> bool {
        user.is_some() && user == self.owner
    }

    /// The `OpenRouter` model id that writes the captions.
    pub fn model(&self) -> &str {
        self.client.model()
    }

    /// How long media waits for a caption before it is sent with a static one.
    pub const fn initial_wait(&self) -> Duration {
        self.initial_wait
    }

    /// Number of leading media items the model will look at, so callers only
    /// fetch those.
    pub fn visual_item_count(&self, kinds: impl IntoIterator<Item = MediaKind>) -> usize {
        self.tools.plan(kinds).len()
    }

    /// Ask the model for one caption line.
    ///
    /// Media items that cannot be prepared are skipped; the request still
    /// goes out with the remaining context.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the reply is not a usable
    /// caption.
    pub async fn caption(&self, request: &CaptionRequest) -> Result<String> {
        let work_dir = tempdir()?;
        let visuals = self.tools.prepare(&request.media, work_dir.path()).await;
        let style_examples = self.comments.sample(STYLE_EXAMPLE_COUNT);
        let context = PromptContext {
            platform: request.platform,
            sender_name: request.sender_name.as_deref(),
            sender_is_owner: self.is_owner(request.sender_id),
            message_text: request.message_text.as_deref(),
            source_text: request.source_text.as_deref(),
            style_examples: &style_examples,
            visual_count: visuals.len(),
        };

        self.complete(SYSTEM_PROMPT, context.render(), visuals)
            .await
    }

    /// Ask the model for Guenther's one-line answer to a reply.
    ///
    /// Media items that cannot be prepared are skipped; the request still
    /// goes out with the remaining context.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the answer is not usable.
    pub async fn reply(&self, request: &ReplyRequest) -> Result<String> {
        let work_dir = tempdir()?;
        let visuals = self.tools.prepare(&request.media, work_dir.path()).await;
        let style_examples = self.comments.sample(STYLE_EXAMPLE_COUNT);
        let context = ReplyContext {
            conversation: &request.conversation,
            replier_name: request.replier_name.as_deref(),
            replier_is_owner: self.is_owner(request.replier_id),
            reply_text: &request.reply_text,
            linked_page: request.linked_page.as_ref(),
            style_examples: &style_examples,
            visual_count: visuals.len(),
        };
        self.complete(REPLY_SYSTEM_PROMPT, context.render(), visuals)
            .await
    }

    async fn complete(
        &self,
        system: &str,
        context: String,
        visuals: Vec<LabeledPart>,
    ) -> Result<String> {
        let mut parts = vec![ContentPart::text(context)];
        for visual in visuals {
            parts.push(ContentPart::text(visual.label));
            parts.push(visual.part);
        }

        debug!(
            model = self.client.model(),
            part_count = parts.len(),
            "Requesting LLM completion"
        );
        let reply = self.client.complete(system, parts).await?;
        sanitize_caption(&reply).ok_or(LlmError::UnusableCaption(reply))
    }
}
