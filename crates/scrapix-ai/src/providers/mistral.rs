//! Mistral provider using OpenAI-compatible HTTP API

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ChatResponse, LlmProvider, Message, MessageRole};
use crate::client::AiClientError;

const MISTRAL_API_URL: &str = "https://api.mistral.ai/v1/chat/completions";

/// Mistral provider (OpenAI-compatible API)
pub struct MistralProvider {
    client: Client,
    api_key: String,
}

impl MistralProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
        }
    }
}

#[derive(Serialize)]
struct MistralRequest<'a> {
    model: &'a str,
    messages: Vec<MistralMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct MistralMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct MistralResponse {
    choices: Vec<MistralChoice>,
    model: String,
    usage: MistralUsage,
}

#[derive(Deserialize)]
struct MistralChoice {
    message: MistralChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct MistralChoiceMessage {
    content: String,
}

#[derive(Deserialize)]
struct MistralUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize)]
struct MistralError {
    message: Option<String>,
    detail: Option<String>,
}

#[async_trait]
impl LlmProvider for MistralProvider {
    async fn chat(
        &self,
        messages: Vec<Message>,
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError> {
        let api_messages: Vec<MistralMessage> = messages
            .iter()
            .map(|m| MistralMessage {
                role: match m.role {
                    MessageRole::System => "system".to_string(),
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                },
                content: m.content.clone(),
            })
            .collect();

        let request_body = MistralRequest {
            model,
            messages: api_messages,
            max_tokens,
            temperature,
        };

        let response = self
            .client
            .post(MISTRAL_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AiClientError::Config(format!("Mistral request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let error_msg = serde_json::from_str::<MistralError>(&body)
                .map(|e| e.message.or(e.detail).unwrap_or(body.clone()))
                .unwrap_or(body);
            return Err(AiClientError::Config(format!(
                "Mistral API error ({}): {}",
                status, error_msg
            )));
        }

        let resp: MistralResponse = response.json().await.map_err(|e| {
            AiClientError::Config(format!("Failed to parse Mistral response: {}", e))
        })?;

        let choice = resp.choices.first().ok_or(AiClientError::EmptyResponse)?;

        Ok(ChatResponse {
            content: choice.message.content.clone(),
            model: resp.model,
            prompt_tokens: resp.usage.prompt_tokens,
            completion_tokens: resp.usage.completion_tokens,
            total_tokens: resp.usage.total_tokens,
            finish_reason: choice.finish_reason.clone(),
        })
    }
}
