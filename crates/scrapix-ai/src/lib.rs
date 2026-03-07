//! # Scrapix AI
//!
//! AI-powered extraction and enrichment for web crawling.
//!
//! This crate provides AI capabilities for the Scrapix crawler:
//!
//! - **Extraction** - Extract structured data using custom prompts
//! - **Summarization** - Generate summaries of varying lengths and styles
//!
//! Supports multiple LLM providers: OpenAI, Anthropic, Google Gemini, and Mistral.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use scrapix_ai::{AiClient, AiClientConfig, Summarizer, AiExtractor};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create the AI client (defaults to OpenAI)
//!     let config = AiClientConfig {
//!         api_key: std::env::var("OPENAI_API_KEY")?,
//!         ..Default::default()
//!     };
//!     let client = Arc::new(AiClient::new(config)?);
//!
//!     // Or use a different provider
//!     // let config = AiClientConfig {
//!     //     api_key: std::env::var("ANTHROPIC_API_KEY")?,
//!     //     provider: "anthropic".to_string(),
//!     //     ..Default::default()
//!     // };
//!
//!     // Summarize content
//!     let summarizer = Summarizer::with_defaults(client.clone());
//!     let summary = summarizer.summarize("Long content to summarize...").await?;
//!     println!("Summary: {}", summary.summary);
//!
//!     // Extract structured data
//!     let extractor = AiExtractor::with_defaults(client.clone());
//!     let result = extractor.extract_fields(
//!         "Article about technology...",
//!         &[("title", "Article title"), ("author", "Author name")]
//!     ).await?;
//!     println!("Extracted: {:?}", result.data);
//!
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod extraction;
pub mod providers;
pub mod summary;

// Re-export main types from client
pub use client::{AiClient, AiClientConfig, AiUsageEvent, AiUsageReceiver, ChatResponse};

// Re-export extraction types
pub use extraction::{
    AiExtractor, ExtractionConfig, ExtractionResult, ExtractionSchema, FieldDefinition,
    SchemaBuilder, DEFAULT_EXTRACTION_MODEL, DEFAULT_MAX_RESPONSE_TOKENS,
};

// Re-export summary types
pub use summary::{
    Summarizer, SummaryConfig, SummaryConfigBuilder, SummaryLength, SummaryResult, SummaryStyle,
    DEFAULT_SUMMARY_MODEL,
};

use scrapix_core::{Result, ScrapixError};
use std::sync::Arc;

/// Unified AI service providing all AI capabilities
///
/// This struct combines extraction and summarization into a single service.
pub struct AiService {
    client: Arc<AiClient>,
    extractor: Option<AiExtractor>,
    summarizer: Option<Summarizer>,
}

impl AiService {
    /// Create a new AI service with all features enabled
    pub fn new(client: Arc<AiClient>) -> Self {
        Self {
            extractor: Some(AiExtractor::with_defaults(client.clone())),
            summarizer: Some(Summarizer::with_defaults(client.clone())),
            client,
        }
    }

    /// Create a minimal service (no features enabled)
    pub fn minimal(client: Arc<AiClient>) -> Self {
        Self {
            client,
            extractor: None,
            summarizer: None,
        }
    }

    /// Enable extraction with default config
    pub fn with_extraction(mut self) -> Self {
        self.extractor = Some(AiExtractor::with_defaults(self.client.clone()));
        self
    }

    /// Enable extraction with custom config
    pub fn with_extraction_config(mut self, config: ExtractionConfig) -> Self {
        self.extractor = Some(AiExtractor::new(self.client.clone(), config));
        self
    }

    /// Enable summarization with default config
    pub fn with_summarization(mut self) -> Self {
        self.summarizer = Some(Summarizer::with_defaults(self.client.clone()));
        self
    }

    /// Enable summarization with custom config
    pub fn with_summary_config(mut self, config: SummaryConfig) -> Self {
        self.summarizer = Some(Summarizer::new(self.client.clone(), config));
        self
    }

    /// Get the underlying client
    pub fn client(&self) -> &Arc<AiClient> {
        &self.client
    }

    /// Get the extractor (if enabled)
    pub fn extractor(&self) -> Option<&AiExtractor> {
        self.extractor.as_ref()
    }

    /// Get the summarizer (if enabled)
    pub fn summarizer(&self) -> Option<&Summarizer> {
        self.summarizer.as_ref()
    }

    /// Extract data using a custom prompt
    pub async fn extract(&self, content: &str, prompt: &str) -> Result<ExtractionResult> {
        let extractor = self
            .extractor
            .as_ref()
            .ok_or_else(|| ScrapixError::Ai("Extraction not enabled".to_string()))?;

        extractor
            .extract_with_prompt(content, prompt)
            .await
            .map_err(|e| ScrapixError::Ai(e.to_string()))
    }

    /// Extract structured data using a schema
    pub async fn extract_schema(
        &self,
        content: &str,
        schema: &ExtractionSchema,
    ) -> Result<ExtractionResult> {
        let extractor = self
            .extractor
            .as_ref()
            .ok_or_else(|| ScrapixError::Ai("Extraction not enabled".to_string()))?;

        extractor
            .extract_with_schema(content, schema)
            .await
            .map_err(|e| ScrapixError::Ai(e.to_string()))
    }

    /// Summarize content
    pub async fn summarize(&self, content: &str) -> Result<SummaryResult> {
        let summarizer = self
            .summarizer
            .as_ref()
            .ok_or_else(|| ScrapixError::Ai("Summarization not enabled".to_string()))?;

        summarizer
            .summarize(content)
            .await
            .map_err(|e| ScrapixError::Ai(e.to_string()))
    }

    /// Generate a TL;DR
    pub async fn tldr(&self, content: &str) -> Result<String> {
        let summarizer = self
            .summarizer
            .as_ref()
            .ok_or_else(|| ScrapixError::Ai("Summarization not enabled".to_string()))?;

        summarizer
            .tldr(content)
            .await
            .map_err(|e| ScrapixError::Ai(e.to_string()))
    }

    /// Generate a headline
    pub async fn headline(&self, content: &str) -> Result<String> {
        let summarizer = self
            .summarizer
            .as_ref()
            .ok_or_else(|| ScrapixError::Ai("Summarization not enabled".to_string()))?;

        summarizer
            .generate_headline(content)
            .await
            .map_err(|e| ScrapixError::Ai(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_client() -> Arc<AiClient> {
        Arc::new(
            AiClient::new(AiClientConfig {
                api_key: "test-key".to_string(),
                ..Default::default()
            })
            .unwrap(),
        )
    }

    #[test]
    fn test_ai_service_creation() {
        let client = create_test_client();
        let service = AiService::new(client);

        assert!(service.extractor().is_some());
        assert!(service.summarizer().is_some());
    }

    #[test]
    fn test_ai_service_minimal() {
        let client = create_test_client();
        let service = AiService::minimal(client);

        assert!(service.extractor().is_none());
        assert!(service.summarizer().is_none());
    }

    #[test]
    fn test_ai_service_builder() {
        let client = create_test_client();
        let service = AiService::minimal(client).with_summarization();

        assert!(service.extractor().is_none());
        assert!(service.summarizer().is_some());
    }

    #[test]
    fn test_token_counting() {
        let text = "Hello, world!";
        let tokens = AiClient::count_tokens(text, "gpt-4").unwrap();
        assert!(tokens > 0);
        assert!(tokens < 10);
    }
}
