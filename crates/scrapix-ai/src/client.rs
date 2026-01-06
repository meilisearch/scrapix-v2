//! OpenAI/LLM client with rate limiting and retry logic
//!
//! This module provides a wrapper around the OpenAI API client with:
//! - Automatic rate limiting
//! - Exponential backoff retry logic
//! - Token counting and budget management
//! - Support for multiple models

use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
        CreateEmbeddingRequestArgs, EmbeddingInput,
    },
    Client as OpenAIClient,
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
    /// OpenAI API key (or compatible API key)
    pub api_key: String,

    /// Base URL for API (for OpenAI-compatible APIs)
    #[serde(default)]
    pub base_url: Option<String>,

    /// Organization ID (optional)
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

/// Response from an embedding request
#[derive(Debug, Clone)]
pub struct EmbeddingResponse {
    /// The embedding vector
    pub embedding: Vec<f32>,

    /// Model used for embedding
    pub model: String,

    /// Number of tokens in the input
    pub tokens_used: u32,
}

/// Batch embedding response
#[derive(Debug, Clone)]
pub struct BatchEmbeddingResponse {
    /// The embedding vectors (one per input)
    pub embeddings: Vec<Vec<f32>>,

    /// Model used for embedding
    pub model: String,

    /// Total tokens used
    pub total_tokens: u32,
}

/// AI client wrapper with rate limiting and retries
pub struct AiClient {
    client: OpenAIClient<OpenAIConfig>,
    config: AiClientConfig,
    semaphore: Arc<Semaphore>,
}

impl AiClient {
    /// Create a new AI client with the given configuration
    pub fn new(config: AiClientConfig) -> Result<Self, AiClientError> {
        if config.api_key.is_empty() {
            return Err(AiClientError::Config("API key is required".to_string()));
        }

        let mut openai_config = OpenAIConfig::new().with_api_key(&config.api_key);

        if let Some(ref base_url) = config.base_url {
            openai_config = openai_config.with_api_base(base_url);
        }

        if let Some(ref org_id) = config.org_id {
            openai_config = openai_config.with_org_id(org_id);
        }

        let client = OpenAIClient::with_config(openai_config);
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_requests));

        Ok(Self {
            client,
            config,
            semaphore,
        })
    }

    /// Create a client from environment variables
    pub fn from_env() -> Result<Self, AiClientError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| AiClientError::Config("OPENAI_API_KEY environment variable not set".to_string()))?;

        let config = AiClientConfig {
            api_key,
            base_url: std::env::var("OPENAI_API_BASE").ok(),
            org_id: std::env::var("OPENAI_ORG_ID").ok(),
            ..Default::default()
        };

        Self::new(config)
    }

    /// Get a tokenizer for a specific model
    pub fn get_tokenizer(model: &str) -> Result<CoreBPE, AiClientError> {
        // Map model names to tiktoken models
        let tiktoken_model = match model {
            m if m.starts_with("gpt-4") => "gpt-4",
            m if m.starts_with("gpt-3.5") => "gpt-3.5-turbo",
            m if m.contains("embedding") => "text-embedding-ada-002",
            _ => "gpt-4", // Default to gpt-4 tokenizer
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

        // Truncate tokens and decode
        let truncated_tokens: Vec<u32> = tokens.into_iter().take(max_tokens).collect();
        bpe.decode(truncated_tokens)
            .map_err(|e| AiClientError::TokenizerError(e.to_string()))
    }

    /// Send a chat completion request with retry logic
    #[instrument(skip(self, messages), fields(model = %model))]
    pub async fn chat(
        &self,
        messages: Vec<ChatCompletionRequestMessage>,
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let mut attempt = 0;
        let mut last_error: Option<AiClientError> = None;

        while attempt < self.config.max_retries {
            attempt += 1;

            match self.do_chat(&messages, model, max_tokens, temperature).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!(attempt, error = %e, "Chat request failed");

                    // Check if retryable
                    if !Self::is_retryable(&e) {
                        return Err(e);
                    }

                    last_error = Some(e);

                    // Exponential backoff
                    let delay = self.config.retry_delay_ms * 2u64.pow(attempt - 1);
                    debug!(delay_ms = delay, "Retrying after delay");
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }

        Err(last_error.unwrap_or(AiClientError::MaxRetriesExceeded(self.config.max_retries)))
    }

    async fn do_chat(
        &self,
        messages: &[ChatCompletionRequestMessage],
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError> {
        let mut request_builder = CreateChatCompletionRequestArgs::default();
        request_builder.model(model).messages(messages.to_vec());

        if let Some(tokens) = max_tokens {
            request_builder.max_tokens(tokens);
        }

        if let Some(temp) = temperature {
            request_builder.temperature(temp);
        }

        let request = request_builder
            .build()
            .map_err(|e| AiClientError::Config(e.to_string()))?;

        let response = self.client.chat().create(request).await?;

        let choice = response
            .choices
            .first()
            .ok_or(AiClientError::EmptyResponse)?;

        let content = choice
            .message
            .content
            .clone()
            .unwrap_or_default();

        let usage = response.usage.as_ref();

        Ok(ChatResponse {
            content,
            model: response.model,
            prompt_tokens: usage.map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens: usage.map(|u| u.completion_tokens).unwrap_or(0),
            total_tokens: usage.map(|u| u.total_tokens).unwrap_or(0),
            finish_reason: choice.finish_reason.as_ref().map(|r| format!("{:?}", r)),
        })
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
            ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content(system_prompt)
                    .build()
                    .map_err(|e| AiClientError::Config(e.to_string()))?,
            ),
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(user_message)
                    .build()
                    .map_err(|e| AiClientError::Config(e.to_string()))?,
            ),
        ];

        self.chat(messages, model, max_tokens, None).await
    }

    /// Generate embeddings for a single text
    #[instrument(skip(self, text), fields(model = %model, text_len = text.len()))]
    pub async fn embed(
        &self,
        text: &str,
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<EmbeddingResponse, AiClientError> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let mut attempt = 0;
        let mut last_error: Option<AiClientError> = None;

        while attempt < self.config.max_retries {
            attempt += 1;

            match self.do_embed(text, model, dimensions).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!(attempt, error = %e, "Embedding request failed");

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

    async fn do_embed(
        &self,
        text: &str,
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<EmbeddingResponse, AiClientError> {
        let mut request_builder = CreateEmbeddingRequestArgs::default();
        request_builder
            .model(model)
            .input(EmbeddingInput::String(text.to_string()));

        if let Some(dims) = dimensions {
            request_builder.dimensions(dims);
        }

        let request = request_builder
            .build()
            .map_err(|e| AiClientError::Config(e.to_string()))?;

        let response = self.client.embeddings().create(request).await?;

        let embedding_data = response
            .data
            .first()
            .ok_or(AiClientError::EmptyResponse)?;

        Ok(EmbeddingResponse {
            embedding: embedding_data.embedding.clone(),
            model: response.model,
            tokens_used: response.usage.prompt_tokens,
        })
    }

    /// Generate embeddings for multiple texts in a batch
    #[instrument(skip(self, texts), fields(model = %model, batch_size = texts.len()))]
    pub async fn embed_batch(
        &self,
        texts: &[String],
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<BatchEmbeddingResponse, AiClientError> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let mut attempt = 0;
        let mut last_error: Option<AiClientError> = None;

        while attempt < self.config.max_retries {
            attempt += 1;

            match self.do_embed_batch(texts, model, dimensions).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!(attempt, error = %e, "Batch embedding request failed");

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

    async fn do_embed_batch(
        &self,
        texts: &[String],
        model: &str,
        dimensions: Option<u32>,
    ) -> Result<BatchEmbeddingResponse, AiClientError> {
        let mut request_builder = CreateEmbeddingRequestArgs::default();
        request_builder
            .model(model)
            .input(EmbeddingInput::StringArray(texts.to_vec()));

        if let Some(dims) = dimensions {
            request_builder.dimensions(dims);
        }

        let request = request_builder
            .build()
            .map_err(|e| AiClientError::Config(e.to_string()))?;

        let response = self.client.embeddings().create(request).await?;

        // Sort by index to maintain order
        let mut embeddings: Vec<(usize, Vec<f32>)> = response
            .data
            .into_iter()
            .map(|e| (e.index as usize, e.embedding))
            .collect();
        embeddings.sort_by_key(|(idx, _)| *idx);

        Ok(BatchEmbeddingResponse {
            embeddings: embeddings.into_iter().map(|(_, e)| e).collect(),
            model: response.model,
            total_tokens: response.usage.prompt_tokens,
        })
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
        assert!(count < 20); // Should be around 8 tokens
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
}
