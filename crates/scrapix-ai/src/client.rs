//! Multi-provider LLM client with rate limiting and retry logic
//!
//! This module provides a provider-agnostic AI client with:
//! - Support for OpenAI, Anthropic, Gemini, and Mistral
//! - Automatic rate limiting
//! - Exponential backoff retry logic
//! - Token counting and truncation

use crate::providers::{
    anthropic::AnthropicProvider, gemini::GeminiProvider, mistral::MistralProvider,
    openai::OpenAiProvider, ChatResponse as ProviderChatResponse, LlmProvider, Message,
    MessageRole,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tiktoken_rs::{get_bpe_from_model, CoreBPE};
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tracing::{debug, instrument, warn};

/// Errors that can occur during AI operations
#[derive(Debug, Error)]
pub enum AiClientError {
    #[error("OpenAI API error: {0}")]
    OpenAI(#[from] async_openai::error::OpenAIError),

    #[error("Rate limit exceeded, retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },

    #[error("Token limit exceeded: {used} tokens used, {limit} allowed")]
    TokenLimitExceeded { used: usize, limit: usize },

    #[error("Max retries ({0}) exceeded")]
    MaxRetriesExceeded(u32),

    #[error("Invalid model: {0}")]
    InvalidModel(String),

    #[error("Tokenizer error: {0}")]
    TokenizerError(String),

    #[error("Empty response from API")]
    EmptyResponse,

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Configuration for the AI client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiClientConfig {
    /// API key for the selected provider
    pub api_key: String,

    /// Provider name: "openai", "anthropic", "gemini", "mistral"
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Base URL for API (for OpenAI-compatible APIs)
    #[serde(default)]
    pub base_url: Option<String>,

    /// Organization ID (optional, OpenAI only)
    #[serde(default)]
    pub org_id: Option<String>,

    /// Maximum concurrent requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Maximum retries for failed requests
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    /// Base delay for exponential backoff (ms)
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    /// Request timeout (ms)
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_provider() -> String {
    "openai".to_string()
}
fn default_max_concurrent() -> usize {
    10
}
fn default_max_retries() -> u32 {
    3
}
fn default_retry_delay_ms() -> u64 {
    1000
}
fn default_timeout_ms() -> u64 {
    60000
}

impl Default for AiClientConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            provider: default_provider(),
            base_url: None,
            org_id: None,
            max_concurrent_requests: default_max_concurrent(),
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

/// Response from a chat completion request
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// The generated text
    pub content: String,

    /// Model used for generation
    pub model: String,

    /// Number of prompt tokens used
    pub prompt_tokens: u32,

    /// Number of completion tokens used
    pub completion_tokens: u32,

    /// Total tokens used
    pub total_tokens: u32,

    /// Finish reason
    pub finish_reason: Option<String>,
}

impl From<ProviderChatResponse> for ChatResponse {
    fn from(r: ProviderChatResponse) -> Self {
        Self {
            content: r.content,
            model: r.model,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
            finish_reason: r.finish_reason,
        }
    }
}

/// AI client wrapper with rate limiting and retries
pub struct AiClient {
    provider: Box<dyn LlmProvider>,
    config: AiClientConfig,
    semaphore: Arc<Semaphore>,
}

impl AiClient {
    /// Create a new AI client with the given configuration
    pub fn new(config: AiClientConfig) -> Result<Self, AiClientError> {
        if config.api_key.is_empty() {
            return Err(AiClientError::Config("API key is required".to_string()));
        }

        let provider: Box<dyn LlmProvider> = match config.provider.as_str() {
            "openai" => Box::new(OpenAiProvider::new(
                &config.api_key,
                config.base_url.as_deref(),
            )),
            "anthropic" => Box::new(AnthropicProvider::new(&config.api_key)),
            "gemini" => Box::new(GeminiProvider::new(&config.api_key)),
            "mistral" => Box::new(MistralProvider::new(&config.api_key)),
            other => {
                return Err(AiClientError::Config(format!(
                    "Unknown provider '{}'. Supported: openai, anthropic, gemini, mistral",
                    other
                )));
            }
        };

        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_requests));

        Ok(Self {
            provider,
            config,
            semaphore,
        })
    }

    /// Create a client from environment variables.
    ///
    /// Reads `AI_PROVIDER` (default: "openai") and the corresponding API key:
    /// - openai: `OPENAI_API_KEY`
    /// - anthropic: `ANTHROPIC_API_KEY`
    /// - gemini: `GOOGLE_GEMINI_API_KEY`
    /// - mistral: `MISTRAL_API_KEY`
    pub fn from_env() -> Result<Self, AiClientError> {
        let provider = std::env::var("AI_PROVIDER").unwrap_or_else(|_| "openai".to_string());

        let api_key = match provider.as_str() {
            "openai" => std::env::var("OPENAI_API_KEY")
                .map_err(|_| AiClientError::Config("OPENAI_API_KEY not set".to_string()))?,
            "anthropic" => std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| AiClientError::Config("ANTHROPIC_API_KEY not set".to_string()))?,
            "gemini" => std::env::var("GOOGLE_GEMINI_API_KEY")
                .map_err(|_| AiClientError::Config("GOOGLE_GEMINI_API_KEY not set".to_string()))?,
            "mistral" => std::env::var("MISTRAL_API_KEY")
                .map_err(|_| AiClientError::Config("MISTRAL_API_KEY not set".to_string()))?,
            other => {
                return Err(AiClientError::Config(format!(
                    "Unknown AI_PROVIDER '{}'. Supported: openai, anthropic, gemini, mistral",
                    other
                )));
            }
        };

        let config = AiClientConfig {
            api_key,
            provider,
            base_url: std::env::var("OPENAI_API_BASE").ok(),
            org_id: std::env::var("OPENAI_ORG_ID").ok(),
            ..Default::default()
        };

        Self::new(config)
    }

    /// Get a tokenizer for a specific model.
    /// Uses gpt-4 tokenizer as a reasonable approximation for all models.
    pub fn get_tokenizer(model: &str) -> Result<CoreBPE, AiClientError> {
        let tiktoken_model = match model {
            m if m.starts_with("gpt-4") || m.starts_with("gpt-5") => "gpt-4",
            m if m.starts_with("gpt-3.5") => "gpt-3.5-turbo",
            _ => "gpt-4", // Default to gpt-4 tokenizer for non-OpenAI models
        };

        get_bpe_from_model(tiktoken_model)
            .map_err(|e| AiClientError::TokenizerError(e.to_string()))
    }

    /// Count tokens in a text for a specific model
    pub fn count_tokens(text: &str, model: &str) -> Result<usize, AiClientError> {
        let bpe = Self::get_tokenizer(model)?;
        Ok(bpe.encode_with_special_tokens(text).len())
    }

    /// Truncate text to fit within a token limit
    pub fn truncate_to_tokens(text: &str, max_tokens: usize, model: &str) -> Result<String, AiClientError> {
        let bpe = Self::get_tokenizer(model)?;
        let tokens = bpe.encode_with_special_tokens(text);

        if tokens.len() <= max_tokens {
            return Ok(text.to_string());
        }

        let truncated_tokens: Vec<u32> = tokens.into_iter().take(max_tokens).collect();
        bpe.decode(truncated_tokens)
            .map_err(|e| AiClientError::TokenizerError(e.to_string()))
    }

    /// Send a chat completion request with retry logic
    #[instrument(skip(self, messages), fields(model = %model))]
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let mut attempt = 0;
        let mut last_error: Option<AiClientError> = None;

        while attempt < self.config.max_retries {
            attempt += 1;

            match self
                .provider
                .chat(messages.clone(), model, max_tokens, temperature)
                .await
            {
                Ok(response) => return Ok(response.into()),
                Err(e) => {
                    warn!(attempt, error = %e, "Chat request failed");

                    if !Self::is_retryable(&e) {
                        return Err(e);
                    }

                    last_error = Some(e);

                    let delay = self.config.retry_delay_ms * 2u64.pow(attempt - 1);
                    debug!(delay_ms = delay, "Retrying after delay");
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }

        Err(last_error.unwrap_or(AiClientError::MaxRetriesExceeded(self.config.max_retries)))
    }

    /// Simple chat with system and user message
    pub async fn simple_chat(
        &self,
        system_prompt: &str,
        user_message: &str,
        model: &str,
        max_tokens: Option<u32>,
    ) -> Result<ChatResponse, AiClientError> {
        let messages = vec![
            Message {
                role: MessageRole::System,
                content: system_prompt.to_string(),
            },
            Message {
                role: MessageRole::User,
                content: user_message.to_string(),
            },
        ];

        self.chat(messages, model, max_tokens, None).await
    }

    /// Check if an error is retryable
    fn is_retryable(error: &AiClientError) -> bool {
        matches!(
            error,
            AiClientError::RateLimited { .. }
                | AiClientError::OpenAI(async_openai::error::OpenAIError::ApiError(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens() {
        let text = "Hello, world! This is a test.";
        let count = AiClient::count_tokens(text, "gpt-4").unwrap();
        assert!(count > 0);
        assert!(count < 20);
    }

    #[test]
    fn test_truncate_to_tokens() {
        let text = "Hello, world! This is a test of the token truncation functionality.";
        let truncated = AiClient::truncate_to_tokens(text, 5, "gpt-4").unwrap();

        let original_tokens = AiClient::count_tokens(text, "gpt-4").unwrap();
        let truncated_tokens = AiClient::count_tokens(&truncated, "gpt-4").unwrap();

        assert!(truncated_tokens <= 5);
        assert!(truncated_tokens < original_tokens);
    }

    #[test]
    fn test_config_defaults() {
        let config = AiClientConfig::default();
        assert_eq!(config.provider, "openai");
        assert_eq!(config.max_concurrent_requests, 10);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delay_ms, 1000);
        assert_eq!(config.timeout_ms, 60000);
    }

    #[test]
    fn test_client_creation_requires_api_key() {
        let config = AiClientConfig::default();
        let result = AiClient::new(config);
        assert!(matches!(result, Err(AiClientError::Config(_))));
    }

    #[test]
    fn test_client_creation_with_api_key() {
        let config = AiClientConfig {
            api_key: "test-key".to_string(),
            ..Default::default()
        };
        let result = AiClient::new(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unknown_provider() {
        let config = AiClientConfig {
            api_key: "test-key".to_string(),
            provider: "unknown".to_string(),
            ..Default::default()
        };
        let result = AiClient::new(config);
        assert!(matches!(result, Err(AiClientError::Config(_))));
    }

    #[test]
    fn test_all_providers_create() {
        for provider in &["openai", "anthropic", "gemini", "mistral"] {
            let config = AiClientConfig {
                api_key: "test-key".to_string(),
                provider: provider.to_string(),
                ..Default::default()
            };
            let result = AiClient::new(config);
            assert!(result.is_ok(), "Failed to create provider: {}", provider);
        }
    }

    #[test]
    fn test_tokenizer_for_non_openai_models() {
        // All non-OpenAI models should fall back to gpt-4 tokenizer
        let count = AiClient::count_tokens("Hello world", "claude-sonnet-4-6").unwrap();
        assert!(count > 0);
        let count = AiClient::count_tokens("Hello world", "gemini-3").unwrap();
        assert!(count > 0);
        let count = AiClient::count_tokens("Hello world", "mistral-3").unwrap();
        assert!(count > 0);
    }
}
