//! LLM provider abstraction for multi-provider support
//!
//! Supports OpenAI, Anthropic, Google Gemini, and Mistral providers.

pub mod anthropic;
pub mod gemini;
pub mod mistral;
pub mod openai;

use crate::client::AiClientError;
use async_trait::async_trait;

/// Message role in a conversation
#[derive(Debug, Clone)]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// A chat message
#[derive(Debug, Clone)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

/// Normalized chat response from any provider
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub finish_reason: Option<String>,
}

/// Trait for LLM providers
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<Message>,
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatResponse, AiClientError>;
}
