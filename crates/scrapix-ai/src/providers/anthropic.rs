//! Anthropic provider using native HTTP API

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ChatResponse, LlmProvider, Message, MessageRole};
use crate::client::AiClientError;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic provider using the native Messages API
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
        }
    }
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: String,
    usage: AnthropicUsage,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicError {
    error: AnthropicErrorDetail,
}

#[derive(Deserialize)]
struct AnthropicErrorDetail {
    message: String,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat(
        &self,
        messages: Vec<Message>,
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError> {
        let mut system_prompt = None;
        let mut api_messages = Vec::new();

        for msg in &messages {
            match msg.role {
                MessageRole::System => {
                    system_prompt = Some(msg.content.clone());
                }
                MessageRole::User => {
                    api_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: msg.content.clone(),
                    });
                }
                MessageRole::Assistant => {
                    api_messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: msg.content.clone(),
                    });
                }
            }
        }

        let request_body = AnthropicRequest {
            model,
            max_tokens: max_tokens.unwrap_or(4096),
            system: system_prompt,
            messages: api_messages,
            temperature,
        };

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AiClientError::Config(format!("Anthropic request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let error_msg = serde_json::from_str::<AnthropicError>(&body)
                .map(|e| e.error.message)
                .unwrap_or(body);
            return Err(AiClientError::Config(format!(
                "Anthropic API error ({}): {}",
                status, error_msg
            )));
        }

        let resp: AnthropicResponse = response.json().await.map_err(|e| {
            AiClientError::Config(format!("Failed to parse Anthropic response: {}", e))
        })?;

        let content = resp
            .content
            .first()
            .map(|c| c.text.clone())
            .ok_or(AiClientError::EmptyResponse)?;

        Ok(ChatResponse {
            content,
            model: resp.model,
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
            finish_reason: resp.stop_reason,
        })
    }
}
