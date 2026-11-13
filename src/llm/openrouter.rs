use crate::{
    llm::error::{LlmError, Result},
    utils::truncate_with_ellipsis,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc, time::Duration};

const CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// Covers low-effort reasoning tokens as well as the short caption itself.
const MAX_TOKENS: u32 = 2048;
const TEMPERATURE: f32 = 1.0;
const ERROR_BODY_CHARS: usize = 500;

/// Minimal `OpenRouter` chat completions client.
#[derive(Clone)]
pub struct OpenRouterClient {
    http: Client,
    api_key: Arc<str>,
    model: Arc<str>,
}

impl OpenRouterClient {
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(api_key: &str, model: &str) -> Result<Self> {
        let http = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
        Ok(Self {
            http,
            api_key: api_key.into(),
            model: model.into(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Send one system prompt and one multi-part user message, returning the
    /// reply text.
    ///
    /// # Errors
    ///
    /// Returns an error on transport failures, non-success statuses, provider
    /// errors, or replies without text.
    pub async fn complete(&self, system: &str, user: Vec<ContentPart>) -> Result<String> {
        let body = ChatRequest::new(&self.model, system, user);
        let response = self
            .http
            .post(CHAT_COMPLETIONS_URL)
            .bearer_auth(&*self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status,
                message: truncate_with_ellipsis(message.trim(), ERROR_BODY_CHARS),
            });
        }

        response.json::<ChatResponse>().await?.into_text()
    }
}

impl fmt::Debug for OpenRouterClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenRouterClient")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

/// One part of a multimodal user message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: MediaUrl },
    VideoUrl { video_url: MediaUrl },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaUrl {
    pub url: String,
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Inline image bytes as a base64 data URL.
    pub fn image(mime_type: &str, bytes: &[u8]) -> Self {
        Self::ImageUrl {
            image_url: MediaUrl {
                url: data_url(mime_type, bytes),
            },
        }
    }

    /// Inline video bytes as a base64 data URL.
    pub fn video(mime_type: &str, bytes: &[u8]) -> Self {
        Self::VideoUrl {
            video_url: MediaUrl {
                url: data_url(mime_type, bytes),
            },
        }
    }
}

fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", STANDARD.encode(bytes))
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    max_tokens: u32,
    temperature: f32,
    reasoning: Reasoning,
    provider: ProviderPreferences,
}

impl<'a> ChatRequest<'a> {
    const fn new(model: &'a str, system: &'a str, user: Vec<ContentPart>) -> Self {
        Self {
            model,
            messages: [
                ChatMessage::System { content: system },
                ChatMessage::User { content: user },
            ],
            max_tokens: MAX_TOKENS,
            temperature: TEMPERATURE,
            reasoning: Reasoning {
                effort: "low",
                exclude: true,
            },
            provider: ProviderPreferences {
                data_collection: "deny",
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum ChatMessage<'a> {
    System { content: &'a str },
    User { content: Vec<ContentPart> },
}

#[derive(Debug, Serialize)]
struct Reasoning {
    effort: &'static str,
    /// Keep reasoning text out of the reply; it is still billed.
    exclude: bool,
}

/// Route only to providers that do not store or train on request data.
#[derive(Debug, Serialize)]
struct ProviderPreferences {
    data_collection: &'static str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<ResponseMessage>,
    error: Option<ChoiceError>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceError {
    message: String,
}

impl ChatResponse {
    fn into_text(self) -> Result<String> {
        let choice = self
            .choices
            .into_iter()
            .next()
            .ok_or(LlmError::EmptyResponse)?;
        if let Some(error) = choice.error {
            return Err(LlmError::Provider(error.message));
        }
        choice
            .message
            .and_then(|message| message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or(LlmError::EmptyResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_ok, assert_ok_eq};
    use serde_json::{Value, json};
    use std::assert_matches;

    #[test]
    fn request_serializes_to_openrouter_schema() {
        let request = ChatRequest::new(
            "z-ai/glm-5.3-flash",
            "system prompt",
            vec![
                ContentPart::text("context"),
                ContentPart::image("image/jpeg", b"jpg"),
                ContentPart::video("video/mp4", b"mp4"),
            ],
        );

        assert_eq!(
            assert_ok!(serde_json::to_value(&request)),
            json!({
                "model": "z-ai/glm-5.3-flash",
                "messages": [
                    { "role": "system", "content": "system prompt" },
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": "context" },
                            { "type": "image_url", "image_url": { "url": "data:image/jpeg;base64,anBn" } },
                            { "type": "video_url", "video_url": { "url": "data:video/mp4;base64,bXA0" } },
                        ],
                    },
                ],
                "max_tokens": 2048,
                "temperature": 1.0,
                "reasoning": { "effort": "low", "exclude": true },
                "provider": { "data_collection": "deny" },
            })
        );
    }

    fn parse(body: &Value) -> Result<String> {
        assert_ok!(serde_json::from_value::<ChatResponse>(body.clone())).into_text()
    }

    #[test]
    fn response_text_is_first_choice_content() {
        let body = json!({
            "choices": [{ "message": { "role": "assistant", "content": "Box. Now." } }],
        });
        assert_ok_eq!(parse(&body), "Box. Now.");
    }

    #[test]
    fn response_choice_error_is_reported() {
        let body = json!({
            "choices": [{ "error": { "code": 502, "message": "upstream died" } }],
        });
        assert_matches!(parse(&body), Err(LlmError::Provider(message)) if message == "upstream died");
    }

    #[test]
    fn response_without_text_is_empty() {
        for body in [
            json!({
                "choices": []
            }),
            json!({
                "choices": [
                    {
                        "message": {
                            "content": null
                        }
                    }
                ]
            }),
            json!({
                "choices": [
                    {
                        "message": {
                            "content": "  \n"
                        }
                    }
                ]
            }),
        ] {
            assert_matches!(parse(&body), Err(LlmError::EmptyResponse));
        }
    }
}
