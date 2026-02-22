//! Google Gemini provider using native HTTP API

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ChatResponse, LlmProvider, Message, MessageRole};
use crate::client::AiClientError;

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Google Gemini provider
pub struct GeminiProvider {
    client: Client,
    api_key: String,
}

impl GeminiProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
    model_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: GeminiContentResponse,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    #[serde(default)]
    prompt_token_count: u32,
    #[serde(default)]
    candidates_token_count: u32,
    #[serde(default)]
    total_token_count: u32,
}

#[derive(Deserialize)]
struct GeminiError {
    error: GeminiErrorDetail,
}

#[derive(Deserialize)]
struct GeminiErrorDetail {
    message: String,
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn chat(
        &self,
        messages: Vec<Message>,
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError> {
        let mut system_instruction = None;
        let mut contents = Vec::new();

        for msg in &messages {
            match msg.role {
                MessageRole::System => {
                    system_instruction = Some(GeminiSystemInstruction {
                        parts: vec![GeminiPart {
                            text: msg.content.clone(),
                        }],
                    });
                }
                MessageRole::User => {
                    contents.push(GeminiContent {
                        role: "user".to_string(),
                        parts: vec![GeminiPart {
                            text: msg.content.clone(),
                        }],
                    });
                }
                MessageRole::Assistant => {
                    contents.push(GeminiContent {
                        role: "model".to_string(),
                        parts: vec![GeminiPart {
                            text: msg.content.clone(),
                        }],
                    });
                }
            }
        }

        let generation_config = if max_tokens.is_some() || temperature.is_some() {
            Some(GeminiGenerationConfig {
                max_output_tokens: max_tokens,
                temperature,
            })
        } else {
            None
        };

        let request_body = GeminiRequest {
            contents,
            system_instruction,
            generation_config,
        };

        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_URL, model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AiClientError::Config(format!("Gemini request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let error_msg = serde_json::from_str::<GeminiError>(&body)
                .map(|e| e.error.message)
                .unwrap_or(body);
            return Err(AiClientError::Config(format!(
                "Gemini API error ({}): {}",
                status, error_msg
            )));
        }

        let resp: GeminiResponse = response
            .json()
            .await
            .map_err(|e| AiClientError::Config(format!("Failed to parse Gemini response: {}", e)))?;

        let candidate = resp
            .candidates
            .first()
            .ok_or(AiClientError::EmptyResponse)?;

        let content = candidate
            .content
            .parts
            .first()
            .map(|p| p.text.clone())
            .ok_or(AiClientError::EmptyResponse)?;

        let usage = resp.usage_metadata.unwrap_or(GeminiUsageMetadata {
            prompt_token_count: 0,
            candidates_token_count: 0,
            total_token_count: 0,
        });

        Ok(ChatResponse {
            content,
            model: resp.model_version.unwrap_or_else(|| model.to_string()),
            prompt_tokens: usage.prompt_token_count,
            completion_tokens: usage.candidates_token_count,
            total_tokens: usage.total_token_count,
            finish_reason: candidate.finish_reason.clone(),
        })
    }
}
