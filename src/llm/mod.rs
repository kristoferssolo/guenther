//! LLM-written media captions via `OpenRouter`.
//!
//! Media is always sent first with a `comments.txt` caption; a [`Captioner`]
//! result only replaces that caption afterwards, so failures here never block
//! delivery.

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
    llm::{
        media::MediaTools,
        openrouter::{ContentPart, OpenRouterClient},
        prompt::{PromptContext, STYLE_EXAMPLE_COUNT, SYSTEM_PROMPT, sanitize_caption},
    },
    utils::MediaKind,
};
use std::sync::Arc;
use tempfile::tempdir;
use tracing::debug;

/// Everything the model sees about one shared link.
#[derive(Debug, Clone)]
pub struct CaptionRequest {
    pub platform: Platform,
    pub sender_name: Option<String>,
    pub message_text: Option<String>,
    pub source_text: Option<String>,
    pub media: Vec<LocalMedia>,
}

/// Writes captions in Guenther's voice from post context and media.
#[derive(Debug, Clone)]
pub struct Captioner {
    client: OpenRouterClient,
    tools: MediaTools,
    comments: Arc<Comments>,
}

impl Captioner {
    /// Build a captioner that takes its style examples from `comments`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(config: &OpenRouterConfig, comments: Arc<Comments>) -> Result<Self> {
        Ok(Self {
            client: OpenRouterClient::new(&config.api_key, &config.model)?,
            tools: MediaTools::new(&config.ffmpeg, &config.ffprobe, config.video_input),
            comments,
        })
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
            message_text: request.message_text.as_deref(),
            source_text: request.source_text.as_deref(),
            style_examples: &style_examples,
            visual_count: visuals.len(),
        };

        let mut parts = vec![ContentPart::text(context.render())];
        for visual in visuals {
            parts.push(ContentPart::text(visual.label));
            parts.push(visual.part);
        }

        debug!(
            model = self.client.model(),
            part_count = parts.len(),
            "Requesting LLM caption"
        );
        let reply = self.client.complete(SYSTEM_PROMPT, parts).await?;
        sanitize_caption(&reply).ok_or(LlmError::UnusableCaption(reply))
    }
}
