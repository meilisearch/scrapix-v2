//! Vector embedding generation for document content
//!
//! This module provides high-level APIs for generating vector embeddings:
//! - Single document embedding
//! - Batch embedding with automatic chunking
//! - Configurable models and dimensions
//! - Token-aware text truncation

use crate::client::{AiClient, AiClientError};
use scrapix_core::config::EmbeddingsConfig;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument};

/// Default embedding model
pub const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

/// Maximum tokens for text-embedding-3-small
pub const MAX_EMBEDDING_TOKENS: usize = 8191;

/// Maximum batch size for embedding requests
pub const MAX_BATCH_SIZE: usize = 2048;

/// Errors that can occur during embedding generation
#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("AI client error: {0}")]
    Client(#[from] AiClientError),

    #[error("Text is empty after preprocessing")]
    EmptyText,

    #[error("Batch is empty")]
    EmptyBatch,

    #[error("Text exceeds maximum token limit ({tokens} > {max})")]
    TextTooLong { tokens: usize, max: usize },

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Configuration for embedding generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Embedding model to use
    #[serde(default = "default_model")]
    pub model: String,

    /// Vector dimensions (if model supports variable dimensions)
    #[serde(default)]
    pub dimensions: Option<u32>,

    /// Maximum tokens per text (will truncate if exceeded)
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Whether to preprocess text (normalize whitespace, etc.)
    #[serde(default = "default_true")]
    pub preprocess: bool,

    /// Batch size for batch embedding requests
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_model() -> String {
    DEFAULT_EMBEDDING_MODEL.to_string()
}

fn default_max_tokens() -> usize {
    MAX_EMBEDDING_TOKENS
}

fn default_true() -> bool {
    true
}

fn default_batch_size() -> usize {
    100
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            dimensions: None,
            max_tokens: default_max_tokens(),
            preprocess: true,
            batch_size: default_batch_size(),
        }
    }
}

impl From<&EmbeddingsConfig> for EmbeddingConfig {
    fn from(config: &EmbeddingsConfig) -> Self {
        Self {
            model: config.model.clone(),
            dimensions: config.dimensions,
            ..Default::default()
        }
    }
}

/// Result of embedding a document
#[derive(Debug, Clone, Serialize)]
pub struct DocumentEmbedding {
    /// The embedding vector
    pub vector: Vec<f32>,

    /// Dimensionality of the vector
    pub dimensions: usize,

    /// Model used for embedding
    pub model: String,

    /// Number of tokens in the input text
    pub tokens_used: u32,

    /// Whether the text was truncated
    pub truncated: bool,
}

/// Result of embedding multiple documents
#[derive(Debug, Clone, Serialize)]
pub struct BatchDocumentEmbedding {
    /// The embedding vectors (one per input)
    pub vectors: Vec<Vec<f32>>,

    /// Dimensionality of each vector
    pub dimensions: usize,

    /// Model used for embedding
    pub model: String,

    /// Total tokens used
    pub total_tokens: u32,

    /// Indices of texts that were truncated
    pub truncated_indices: Vec<usize>,
}

/// Embedding generator service
pub struct EmbeddingGenerator {
    client: Arc<AiClient>,
    config: EmbeddingConfig,
}

impl EmbeddingGenerator {
    /// Create a new embedding generator
    pub fn new(client: Arc<AiClient>, config: EmbeddingConfig) -> Self {
        Self { client, config }
    }

    /// Create with default configuration
    pub fn with_defaults(client: Arc<AiClient>) -> Self {
        Self::new(client, EmbeddingConfig::default())
    }

    /// Create from scrapix-core config
    pub fn from_config(client: Arc<AiClient>, config: &EmbeddingsConfig) -> Self {
        Self::new(client, EmbeddingConfig::from(config))
    }

    /// Get the current configuration
    pub fn config(&self) -> &EmbeddingConfig {
        &self.config
    }

    /// Preprocess text for embedding
    fn preprocess_text(&self, text: &str) -> String {
        if !self.config.preprocess {
            return text.to_string();
        }

        // Normalize whitespace
        let text = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        // Remove control characters except newlines
        text.chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect()
    }

    /// Truncate text to fit within token limit
    fn truncate_if_needed(&self, text: &str) -> Result<(String, bool), EmbeddingError> {
        let tokens = AiClient::count_tokens(text, &self.config.model)?;

        if tokens <= self.config.max_tokens {
            return Ok((text.to_string(), false));
        }

        let truncated = AiClient::truncate_to_tokens(text, self.config.max_tokens, &self.config.model)?;
        Ok((truncated, true))
    }

    /// Generate embedding for a single text
    #[instrument(skip(self, text), fields(text_len = text.len()))]
    pub async fn embed(&self, text: &str) -> Result<DocumentEmbedding, EmbeddingError> {
        // Preprocess
        let processed = self.preprocess_text(text);

        if processed.is_empty() {
            return Err(EmbeddingError::EmptyText);
        }

        // Truncate if needed
        let (final_text, truncated) = self.truncate_if_needed(&processed)?;

        debug!(
            model = %self.config.model,
            truncated = truncated,
            "Generating embedding"
        );

        // Generate embedding
        let response = self
            .client
            .embed(&final_text, &self.config.model, self.config.dimensions)
            .await?;

        Ok(DocumentEmbedding {
            dimensions: response.embedding.len(),
            vector: response.embedding,
            model: response.model,
            tokens_used: response.tokens_used,
            truncated,
        })
    }

    /// Generate embeddings for multiple texts
    #[instrument(skip(self, texts), fields(batch_size = texts.len()))]
    pub async fn embed_batch(&self, texts: &[String]) -> Result<BatchDocumentEmbedding, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::EmptyBatch);
        }

        // Preprocess and truncate all texts
        let mut processed_texts = Vec::with_capacity(texts.len());
        let mut truncated_indices = Vec::new();

        for (idx, text) in texts.iter().enumerate() {
            let processed = self.preprocess_text(text);

            if processed.is_empty() {
                // Use a placeholder for empty texts
                processed_texts.push(" ".to_string());
                continue;
            }

            let (final_text, truncated) = self.truncate_if_needed(&processed)?;
            if truncated {
                truncated_indices.push(idx);
            }
            processed_texts.push(final_text);
        }

        debug!(
            model = %self.config.model,
            batch_size = texts.len(),
            truncated_count = truncated_indices.len(),
            "Generating batch embeddings"
        );

        // Process in chunks if batch is too large
        if processed_texts.len() <= self.config.batch_size {
            let response = self
                .client
                .embed_batch(&processed_texts, &self.config.model, self.config.dimensions)
                .await?;

            return Ok(BatchDocumentEmbedding {
                dimensions: response.embeddings.first().map(|e| e.len()).unwrap_or(0),
                vectors: response.embeddings,
                model: response.model,
                total_tokens: response.total_tokens,
                truncated_indices,
            });
        }

        // Process in batches
        let mut all_embeddings = Vec::with_capacity(processed_texts.len());
        let mut total_tokens = 0u32;
        let mut model = String::new();
        let mut dimensions = 0;

        for chunk in processed_texts.chunks(self.config.batch_size) {
            let response = self
                .client
                .embed_batch(&chunk.to_vec(), &self.config.model, self.config.dimensions)
                .await?;

            if dimensions == 0 {
                dimensions = response.embeddings.first().map(|e| e.len()).unwrap_or(0);
                model = response.model.clone();
            }

            total_tokens += response.total_tokens;
            all_embeddings.extend(response.embeddings);
        }

        Ok(BatchDocumentEmbedding {
            vectors: all_embeddings,
            dimensions,
            model,
            total_tokens,
            truncated_indices,
        })
    }

    /// Generate embedding for document content with optional title weighting
    #[instrument(skip(self, content, title))]
    pub async fn embed_document(
        &self,
        content: &str,
        title: Option<&str>,
    ) -> Result<DocumentEmbedding, EmbeddingError> {
        // Combine title and content with title getting emphasis
        let text = match title {
            Some(t) if !t.is_empty() => format!("{}\n\n{}", t, content),
            _ => content.to_string(),
        };

        self.embed(&text).await
    }

    /// Estimate tokens for a text without making an API call
    pub fn estimate_tokens(&self, text: &str) -> Result<usize, EmbeddingError> {
        AiClient::count_tokens(text, &self.config.model).map_err(EmbeddingError::from)
    }

    /// Check if text will need truncation
    pub fn will_truncate(&self, text: &str) -> Result<bool, EmbeddingError> {
        let tokens = self.estimate_tokens(text)?;
        Ok(tokens > self.config.max_tokens)
    }
}

/// Similarity calculations for embeddings
pub mod similarity {
    /// Calculate cosine similarity between two vectors
    pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot / (norm_a * norm_b)
    }

    /// Calculate Euclidean distance between two vectors
    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return f32::MAX;
        }

        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Calculate dot product between two vectors
    pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Find the most similar vectors from a collection
    pub fn find_most_similar(
        query: &[f32],
        candidates: &[Vec<f32>],
        top_k: usize,
    ) -> Vec<(usize, f32)> {
        let mut similarities: Vec<(usize, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(idx, candidate)| (idx, cosine(query, candidate)))
            .collect();

        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        similarities.truncate(top_k);
        similarities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.model, DEFAULT_EMBEDDING_MODEL);
        assert_eq!(config.max_tokens, MAX_EMBEDDING_TOKENS);
        assert!(config.preprocess);
    }

    #[test]
    fn test_from_embeddings_config() {
        let core_config = EmbeddingsConfig {
            enabled: true,
            model: "text-embedding-ada-002".to_string(),
            dimensions: Some(1536),
            include_pages: vec![],
            exclude_pages: vec![],
        };

        let config = EmbeddingConfig::from(&core_config);
        assert_eq!(config.model, "text-embedding-ada-002");
        assert_eq!(config.dimensions, Some(1536));
    }

    mod similarity_tests {
        use super::similarity::*;

        #[test]
        fn test_cosine_similarity_identical() {
            let a = vec![1.0, 0.0, 0.0];
            let b = vec![1.0, 0.0, 0.0];
            let sim = cosine(&a, &b);
            assert!((sim - 1.0).abs() < 1e-6);
        }

        #[test]
        fn test_cosine_similarity_orthogonal() {
            let a = vec![1.0, 0.0, 0.0];
            let b = vec![0.0, 1.0, 0.0];
            let sim = cosine(&a, &b);
            assert!(sim.abs() < 1e-6);
        }

        #[test]
        fn test_cosine_similarity_opposite() {
            let a = vec![1.0, 0.0, 0.0];
            let b = vec![-1.0, 0.0, 0.0];
            let sim = cosine(&a, &b);
            assert!((sim + 1.0).abs() < 1e-6);
        }

        #[test]
        fn test_euclidean_distance() {
            let a = vec![0.0, 0.0, 0.0];
            let b = vec![3.0, 4.0, 0.0];
            let dist = euclidean_distance(&a, &b);
            assert!((dist - 5.0).abs() < 1e-6);
        }

        #[test]
        fn test_dot_product() {
            let a = vec![1.0, 2.0, 3.0];
            let b = vec![4.0, 5.0, 6.0];
            let dot = dot_product(&a, &b);
            assert!((dot - 32.0).abs() < 1e-6); // 1*4 + 2*5 + 3*6 = 32
        }

        #[test]
        fn test_find_most_similar() {
            let query = vec![1.0, 0.0];
            let candidates = vec![
                vec![1.0, 0.0],   // identical
                vec![0.0, 1.0],   // orthogonal
                vec![0.7, 0.7],   // similar
                vec![-1.0, 0.0],  // opposite
            ];

            let results = find_most_similar(&query, &candidates, 2);
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].0, 0); // Most similar is identical
            assert_eq!(results[1].0, 2); // Second most similar
        }
    }
}
