//! Content summarization using LLMs
//!
//! This module provides content summarization capabilities:
//! - Configurable summary length (short, medium, long)
//! - Different summarization styles (paragraph, bullet points, key points)
//! - Multi-document summarization
//! - Headline and title generation

use crate::client::{AiClient, AiClientError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument};

/// Default model for summarization
pub const DEFAULT_SUMMARY_MODEL: &str = "gpt-4o-mini";

/// Errors that can occur during summarization
#[derive(Debug, Error)]
pub enum SummaryError {
    #[error("AI client error: {0}")]
    Client(#[from] AiClientError),

    #[error("Content is empty")]
    EmptyContent,

    #[error("Summarization failed: {0}")]
    SummaryFailed(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Summary length options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryLength {
    /// Very short summary (1-2 sentences, ~50 words)
    Short,

    /// Medium length summary (1 paragraph, ~100-150 words)
    #[default]
    Medium,

    /// Detailed summary (2-3 paragraphs, ~250-300 words)
    Long,

    /// Custom word limit
    Custom(u32),
}

impl SummaryLength {
    /// Get the approximate word count for this length
    pub fn word_count(&self) -> u32 {
        match self {
            SummaryLength::Short => 50,
            SummaryLength::Medium => 150,
            SummaryLength::Long => 300,
            SummaryLength::Custom(words) => *words,
        }
    }

    /// Get description for prompts
    pub fn description(&self) -> String {
        match self {
            SummaryLength::Short => "1-2 sentences (about 50 words)".to_string(),
            SummaryLength::Medium => "1 paragraph (about 150 words)".to_string(),
            SummaryLength::Long => "2-3 paragraphs (about 300 words)".to_string(),
            SummaryLength::Custom(words) => format!("approximately {} words", words),
        }
    }
}

/// Summary style options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStyle {
    /// Flowing paragraph summary
    #[default]
    Paragraph,

    /// Bullet point summary
    BulletPoints,

    /// Key points with brief explanations
    KeyPoints,

    /// Executive summary style
    Executive,

    /// Technical summary focusing on details
    Technical,
}

impl SummaryStyle {
    /// Get formatting instructions for this style
    pub fn instructions(&self) -> &str {
        match self {
            SummaryStyle::Paragraph => {
                "Write the summary as flowing prose in paragraph form."
            }
            SummaryStyle::BulletPoints => {
                "Write the summary as a bulleted list of main points."
            }
            SummaryStyle::KeyPoints => {
                "Write the summary as numbered key points, each with a brief explanation."
            }
            SummaryStyle::Executive => {
                "Write an executive summary highlighting key findings, implications, and recommendations."
            }
            SummaryStyle::Technical => {
                "Write a technical summary focusing on specific details, data, and methodology."
            }
        }
    }
}

/// Configuration for summarization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryConfig {
    /// Model to use for summarization
    #[serde(default = "default_model")]
    pub model: String,

    /// Desired summary length
    #[serde(default)]
    pub length: SummaryLength,

    /// Summary style
    #[serde(default)]
    pub style: SummaryStyle,

    /// Maximum tokens for the response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Maximum content tokens to send
    #[serde(default = "default_max_content_tokens")]
    pub max_content_tokens: usize,

    /// Temperature for generation
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Additional instructions to include in the prompt
    #[serde(default)]
    pub custom_instructions: Option<String>,
}

fn default_model() -> String {
    DEFAULT_SUMMARY_MODEL.to_string()
}

fn default_max_tokens() -> u32 {
    1024
}

fn default_max_content_tokens() -> usize {
    6000
}

fn default_temperature() -> f32 {
    0.3 // Slightly creative but mostly factual
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            length: SummaryLength::default(),
            style: SummaryStyle::default(),
            max_tokens: default_max_tokens(),
            max_content_tokens: default_max_content_tokens(),
            temperature: default_temperature(),
            custom_instructions: None,
        }
    }
}

/// Result of summarization
#[derive(Debug, Clone, Serialize)]
pub struct SummaryResult {
    /// The generated summary
    pub summary: String,

    /// Model used for summarization
    pub model: String,

    /// Tokens used for the prompt
    pub prompt_tokens: u32,

    /// Tokens used for the response
    pub completion_tokens: u32,

    /// Whether content was truncated
    pub truncated: bool,

    /// Approximate word count of the summary
    pub word_count: usize,
}

/// Content summarizer service
pub struct Summarizer {
    client: Arc<AiClient>,
    config: SummaryConfig,
}

impl Summarizer {
    /// Create a new summarizer
    pub fn new(client: Arc<AiClient>, config: SummaryConfig) -> Self {
        Self { client, config }
    }

    /// Create with default configuration
    pub fn with_defaults(client: Arc<AiClient>) -> Self {
        Self::new(client, SummaryConfig::default())
    }

    /// Get the current configuration
    pub fn config(&self) -> &SummaryConfig {
        &self.config
    }

    /// Truncate content to fit within token limits
    fn truncate_content(&self, content: &str) -> Result<(String, bool), SummaryError> {
        let tokens = AiClient::count_tokens(content, &self.config.model)?;

        if tokens <= self.config.max_content_tokens {
            return Ok((content.to_string(), false));
        }

        let truncated = AiClient::truncate_to_tokens(
            content,
            self.config.max_content_tokens,
            &self.config.model,
        )?;

        Ok((truncated, true))
    }

    /// Build the system prompt for summarization
    fn build_prompt(&self) -> String {
        let mut prompt = format!(
            "You are a skilled summarizer. Create a summary that is {}.\n\n{}",
            self.config.length.description(),
            self.config.style.instructions()
        );

        if let Some(ref instructions) = self.config.custom_instructions {
            prompt.push_str(&format!("\n\nAdditional instructions: {}", instructions));
        }

        prompt.push_str("\n\nProvide only the summary, without any preamble or explanation.");

        prompt
    }

    /// Summarize content
    #[instrument(skip(self, content), fields(content_len = content.len()))]
    pub async fn summarize(&self, content: &str) -> Result<SummaryResult, SummaryError> {
        if content.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        let (truncated_content, was_truncated) = self.truncate_content(content)?;

        let system_prompt = self.build_prompt();
        let user_message = format!("Summarize the following content:\n\n{}", truncated_content);

        debug!(
            model = %self.config.model,
            length = ?self.config.length,
            style = ?self.config.style,
            truncated = was_truncated,
            "Generating summary"
        );

        let response = self
            .client
            .simple_chat(
                &system_prompt,
                &user_message,
                &self.config.model,
                Some(self.config.max_tokens),
            )
            .await?;

        let word_count = response.content.split_whitespace().count();

        Ok(SummaryResult {
            summary: response.content,
            model: response.model,
            prompt_tokens: response.prompt_tokens,
            completion_tokens: response.completion_tokens,
            truncated: was_truncated,
            word_count,
        })
    }

    /// Summarize with a specific length
    pub async fn summarize_to_length(
        &self,
        content: &str,
        length: SummaryLength,
    ) -> Result<SummaryResult, SummaryError> {
        let mut config = self.config.clone();
        config.length = length;

        let summarizer = Summarizer::new(self.client.clone(), config);
        summarizer.summarize(content).await
    }

    /// Summarize with a specific style
    pub async fn summarize_with_style(
        &self,
        content: &str,
        style: SummaryStyle,
    ) -> Result<SummaryResult, SummaryError> {
        let mut config = self.config.clone();
        config.style = style;

        let summarizer = Summarizer::new(self.client.clone(), config);
        summarizer.summarize(content).await
    }

    /// Generate a headline/title for the content
    #[instrument(skip(self, content), fields(content_len = content.len()))]
    pub async fn generate_headline(&self, content: &str) -> Result<String, SummaryError> {
        if content.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        let (truncated_content, _) = self.truncate_content(content)?;

        let system_prompt = "Generate a concise, engaging headline or title for the following content. \
                            The headline should be informative and capture the main point. \
                            Respond with only the headline, no quotes or explanation.";

        let response = self
            .client
            .simple_chat(
                system_prompt,
                &truncated_content,
                &self.config.model,
                Some(50), // Headlines should be short
            )
            .await?;

        // Clean up the headline
        let headline = response
            .content
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        Ok(headline)
    }

    /// Generate key takeaways from the content
    #[instrument(skip(self, content), fields(content_len = content.len()))]
    pub async fn extract_key_takeaways(
        &self,
        content: &str,
        count: usize,
    ) -> Result<Vec<String>, SummaryError> {
        if content.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        let (truncated_content, _) = self.truncate_content(content)?;

        let system_prompt = format!(
            "Extract exactly {} key takeaways from the following content. \
             Each takeaway should be a single, concise sentence. \
             Return ONLY the takeaways, one per line, without numbering or bullet points.",
            count
        );

        let response = self
            .client
            .simple_chat(
                &system_prompt,
                &truncated_content,
                &self.config.model,
                Some(self.config.max_tokens),
            )
            .await?;

        let takeaways: Vec<String> = response
            .content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| {
                // Remove common prefixes
                line.trim_start_matches(|c: char| c.is_numeric() || c == '.' || c == '-' || c == '*' || c == '•')
                    .trim()
                    .to_string()
            })
            .take(count)
            .collect();

        Ok(takeaways)
    }

    /// Summarize multiple documents into a single summary
    #[instrument(skip(self, documents), fields(doc_count = documents.len()))]
    pub async fn summarize_multiple(
        &self,
        documents: &[&str],
    ) -> Result<SummaryResult, SummaryError> {
        if documents.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        // First, summarize each document individually to reduce size
        let mut individual_summaries = Vec::with_capacity(documents.len());

        for doc in documents {
            if doc.is_empty() {
                continue;
            }

            // Create a shorter summary for each document
            let short_config = SummaryConfig {
                length: SummaryLength::Short,
                ..self.config.clone()
            };

            let summarizer = Summarizer::new(self.client.clone(), short_config);
            match summarizer.summarize(doc).await {
                Ok(result) => individual_summaries.push(result.summary),
                Err(e) => {
                    debug!(error = %e, "Failed to summarize individual document");
                    // Skip failed documents
                }
            }
        }

        if individual_summaries.is_empty() {
            return Err(SummaryError::SummaryFailed(
                "All documents failed to summarize".to_string(),
            ));
        }

        // Combine summaries and create a final summary
        let combined = individual_summaries.join("\n\n---\n\n");

        let system_prompt = format!(
            "You are given summaries of {} related documents. \
             Create a unified summary that synthesizes the information from all documents. \
             The summary should be {} and {}.\n\n\
             Highlight common themes and key differences between the documents.",
            documents.len(),
            self.config.length.description(),
            self.config.style.instructions()
        );

        let (truncated_combined, was_truncated) = self.truncate_content(&combined)?;

        let response = self
            .client
            .simple_chat(
                &system_prompt,
                &truncated_combined,
                &self.config.model,
                Some(self.config.max_tokens),
            )
            .await?;

        let word_count = response.content.split_whitespace().count();

        Ok(SummaryResult {
            summary: response.content,
            model: response.model,
            prompt_tokens: response.prompt_tokens,
            completion_tokens: response.completion_tokens,
            truncated: was_truncated,
            word_count,
        })
    }

    /// Generate a TL;DR (very short summary)
    #[instrument(skip(self, content), fields(content_len = content.len()))]
    pub async fn tldr(&self, content: &str) -> Result<String, SummaryError> {
        if content.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        let (truncated_content, _) = self.truncate_content(content)?;

        let system_prompt = "Create a TL;DR (too long; didn't read) summary of the following content. \
                            This should be a single sentence that captures the absolute essence. \
                            Respond with only the TL;DR, no prefix or explanation.";

        let response = self
            .client
            .simple_chat(
                system_prompt,
                &truncated_content,
                &self.config.model,
                Some(100), // TL;DR should be very short
            )
            .await?;

        Ok(response.content.trim().to_string())
    }

    /// Summarize for a specific audience
    #[instrument(skip(self, content))]
    pub async fn summarize_for_audience(
        &self,
        content: &str,
        audience: &str,
    ) -> Result<SummaryResult, SummaryError> {
        if content.is_empty() {
            return Err(SummaryError::EmptyContent);
        }

        let (truncated_content, was_truncated) = self.truncate_content(content)?;

        let system_prompt = format!(
            "Create a summary tailored for the following audience: {}\n\n\
             The summary should be {} and use language and concepts appropriate for this audience.\n\n\
             {}",
            audience,
            self.config.length.description(),
            self.config.style.instructions()
        );

        let response = self
            .client
            .simple_chat(
                &system_prompt,
                &truncated_content,
                &self.config.model,
                Some(self.config.max_tokens),
            )
            .await?;

        let word_count = response.content.split_whitespace().count();

        Ok(SummaryResult {
            summary: response.content,
            model: response.model,
            prompt_tokens: response.prompt_tokens,
            completion_tokens: response.completion_tokens,
            truncated: was_truncated,
            word_count,
        })
    }
}

/// Builder for summary configuration
pub struct SummaryConfigBuilder {
    config: SummaryConfig,
}

impl SummaryConfigBuilder {
    /// Create a new builder with defaults
    pub fn new() -> Self {
        Self {
            config: SummaryConfig::default(),
        }
    }

    /// Set the model
    pub fn model(mut self, model: &str) -> Self {
        self.config.model = model.to_string();
        self
    }

    /// Set summary length
    pub fn length(mut self, length: SummaryLength) -> Self {
        self.config.length = length;
        self
    }

    /// Set short length
    pub fn short(mut self) -> Self {
        self.config.length = SummaryLength::Short;
        self
    }

    /// Set medium length
    pub fn medium(mut self) -> Self {
        self.config.length = SummaryLength::Medium;
        self
    }

    /// Set long length
    pub fn long(mut self) -> Self {
        self.config.length = SummaryLength::Long;
        self
    }

    /// Set custom word count
    pub fn words(mut self, count: u32) -> Self {
        self.config.length = SummaryLength::Custom(count);
        self
    }

    /// Set summary style
    pub fn style(mut self, style: SummaryStyle) -> Self {
        self.config.style = style;
        self
    }

    /// Set bullet points style
    pub fn bullet_points(mut self) -> Self {
        self.config.style = SummaryStyle::BulletPoints;
        self
    }

    /// Set key points style
    pub fn key_points(mut self) -> Self {
        self.config.style = SummaryStyle::KeyPoints;
        self
    }

    /// Set executive style
    pub fn executive(mut self) -> Self {
        self.config.style = SummaryStyle::Executive;
        self
    }

    /// Set technical style
    pub fn technical(mut self) -> Self {
        self.config.style = SummaryStyle::Technical;
        self
    }

    /// Set custom instructions
    pub fn instructions(mut self, instructions: &str) -> Self {
        self.config.custom_instructions = Some(instructions.to_string());
        self
    }

    /// Set temperature
    pub fn temperature(mut self, temp: f32) -> Self {
        self.config.temperature = temp;
        self
    }

    /// Build the configuration
    pub fn build(self) -> SummaryConfig {
        self.config
    }
}

impl Default for SummaryConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = SummaryConfig::default();
        assert_eq!(config.model, DEFAULT_SUMMARY_MODEL);
        assert_eq!(config.length, SummaryLength::Medium);
        assert_eq!(config.style, SummaryStyle::Paragraph);
    }

    #[test]
    fn test_summary_length_word_count() {
        assert_eq!(SummaryLength::Short.word_count(), 50);
        assert_eq!(SummaryLength::Medium.word_count(), 150);
        assert_eq!(SummaryLength::Long.word_count(), 300);
        assert_eq!(SummaryLength::Custom(200).word_count(), 200);
    }

    #[test]
    fn test_config_builder() {
        let config = SummaryConfigBuilder::new()
            .model("gpt-4")
            .short()
            .bullet_points()
            .instructions("Focus on technical details")
            .build();

        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.length, SummaryLength::Short);
        assert_eq!(config.style, SummaryStyle::BulletPoints);
        assert!(config.custom_instructions.is_some());
    }

    #[test]
    fn test_prompt_building() {
        let config = SummaryConfigBuilder::new()
            .medium()
            .key_points()
            .instructions("Include statistics")
            .build();

        let summarizer = Summarizer::new(
            Arc::new(
                AiClient::new(crate::client::AiClientConfig {
                    api_key: "test".to_string(),
                    ..Default::default()
                })
                .unwrap(),
            ),
            config,
        );

        let prompt = summarizer.build_prompt();
        assert!(prompt.contains("150 words"));
        assert!(prompt.contains("key points"));
        assert!(prompt.contains("Include statistics"));
    }
}
