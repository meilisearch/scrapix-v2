//! # Scrapix Extractor
//!
//! Feature extraction from parsed HTML documents.
//!
//! This crate provides extractors for various types of structured data from HTML:
//!
//! - **Metadata extraction** - Meta tags, Open Graph, Twitter Cards, Dublin Core
//! - **Schema.org/JSON-LD extraction** - Structured data in JSON-LD and Microdata formats
//! - **Custom CSS selector extraction** - Extract specific data using CSS selectors
//! - **Block splitting** - Split content into semantic blocks by heading hierarchy
//!
//! ## Quick Start
//!
//! ```rust
//! use scrapix_extractor::{MetadataExtractor, SchemaExtractor, SelectorExtractor, BlockSplitter};
//!
//! let html = r#"
//!     <!DOCTYPE html>
//!     <html>
//!     <head>
//!         <title>Example Page</title>
//!         <meta name="description" content="An example page">
//!         <meta property="og:title" content="OG Title">
//!     </head>
//!     <body>
//!         <h1>Hello World</h1>
//!         <p>Some content here.</p>
//!     </body>
//!     </html>
//! "#;
//!
//! // Extract metadata
//! let metadata_extractor = MetadataExtractor::new();
//! let metadata = metadata_extractor.extract(html).unwrap();
//! assert_eq!(metadata.title, Some("Example Page".to_string()));
//!
//! // Extract schema.org data (if present)
//! let schema_extractor = SchemaExtractor::default();
//! let schema = schema_extractor.extract(html).unwrap();
//!
//! // Extract custom fields with CSS selectors
//! let mut selector_extractor = SelectorExtractor::new();
//! selector_extractor.add_text("heading", "h1");
//! let custom = selector_extractor.extract(html).unwrap();
//! ```
//!
//! ## Feature Modules
//!
//! ### Metadata Extraction
//!
//! The [`metadata`] module extracts standard HTML metadata including:
//! - `<title>` tag
//! - Meta tags (description, keywords, author, robots)
//! - Open Graph tags (og:title, og:description, etc.)
//! - Twitter Card tags
//! - Dublin Core metadata
//! - Canonical URLs, feeds, favicons
//!
//! ### Schema.org Extraction
//!
//! The [`schema`] module extracts structured data in:
//! - JSON-LD format (`<script type="application/ld+json">`)
//! - Microdata format (itemscope, itemprop attributes)
//! - Supports filtering by schema type (Article, Product, Organization, etc.)
//!
//! ### Custom Selectors
//!
//! The [`selectors`] module allows custom data extraction using:
//! - CSS selectors for element matching
//! - Multiple extraction modes (text, HTML, attributes, lists)
//! - Value transformations (trim, parse numbers, regex extract)
//! - Fallback values for missing elements
//!
//! ### Block Splitting
//!
//! The [`blocks`] module splits documents into semantic blocks:
//! - Split on configurable heading levels (H1-H6)
//! - Maintain heading hierarchy for each block
//! - Extract anchor IDs for deep linking
//! - Filter blocks by minimum content length

pub mod blocks;
pub mod metadata;
pub mod schema;
pub mod selectors;

// Re-export main types for convenience
pub use blocks::{BlockConfig, BlockError, BlockSplitter, ContentBlock, ExtractedBlocks};
pub use metadata::{ExtractedMetadata, FeedLink, MetadataError, MetadataExtractor};
pub use schema::{ExtractedSchema, SchemaConfig, SchemaError, SchemaExtractor, SchemaItem};
pub use selectors::{
    ExtractedSelectors, ExtractionMode, FieldDefinition, SelectorDefinition, SelectorError,
    SelectorExtractor, SelectorInput, Transform,
};

use scraper::Html;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

// Re-export scraper::Html so callers that already have a parsed DOM can use it
pub use scraper;

/// Combined error type for all extraction errors
#[derive(Debug, Error)]
pub enum ExtractorError {
    #[error("Metadata extraction error: {0}")]
    Metadata(#[from] MetadataError),

    #[error("Schema extraction error: {0}")]
    Schema(#[from] SchemaError),

    #[error("Selector extraction error: {0}")]
    Selector(#[from] SelectorError),

    #[error("Block extraction error: {0}")]
    Block(#[from] BlockError),
}

/// Combined extraction result
#[derive(Debug, Clone, Default)]
pub struct ExtractionResult {
    /// Extracted metadata
    pub metadata: Option<ExtractedMetadata>,

    /// Extracted schema data
    pub schema: Option<ExtractedSchema>,

    /// Custom selector extractions
    pub custom: Option<ExtractedSelectors>,

    /// Content blocks
    pub blocks: Option<ExtractedBlocks>,
}

impl ExtractionResult {
    /// Convert to a flat HashMap suitable for document storage
    pub fn to_document_fields(&self) -> HashMap<String, Value> {
        let mut fields = HashMap::new();

        // Add metadata
        if let Some(ref meta) = self.metadata {
            if let Some(ref title) = meta.title {
                fields.insert("title".to_string(), Value::String(title.clone()));
            }
            if let Some(ref desc) = meta.description {
                fields.insert("description".to_string(), Value::String(desc.clone()));
            }
            if let Some(ref author) = meta.author {
                fields.insert("author".to_string(), Value::String(author.clone()));
            }
            if !meta.keywords.is_empty() {
                fields.insert(
                    "keywords".to_string(),
                    Value::Array(
                        meta.keywords
                            .iter()
                            .map(|k| Value::String(k.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(ref canonical) = meta.canonical_url {
                fields.insert(
                    "canonical_url".to_string(),
                    Value::String(canonical.clone()),
                );
            }
            if let Some(ref published) = meta.published_date {
                fields.insert(
                    "published_date".to_string(),
                    Value::String(published.clone()),
                );
            }

            // Add Open Graph as nested object
            if !meta.open_graph.is_empty() {
                let og: serde_json::Map<String, Value> = meta
                    .open_graph
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                fields.insert("open_graph".to_string(), Value::Object(og));
            }
        }

        // Add schema
        if let Some(ref schema) = self.schema {
            if let Some(ref json_ld) = schema.json_ld {
                fields.insert("schema".to_string(), json_ld.clone());
            }
        }

        // Add custom extractions
        if let Some(ref custom) = self.custom {
            for (key, value) in &custom.values {
                fields.insert(format!("custom_{}", key), value.clone());
            }
        }

        fields
    }
}

/// Unified extractor that runs multiple extraction pipelines
pub struct Extractor {
    metadata_extractor: Option<MetadataExtractor>,
    schema_extractor: Option<SchemaExtractor>,
    selector_extractor: Option<SelectorExtractor>,
    block_splitter: Option<BlockSplitter>,
}

impl Default for Extractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor {
    /// Create a new extractor with no features enabled
    pub fn new() -> Self {
        Self {
            metadata_extractor: None,
            schema_extractor: None,
            selector_extractor: None,
            block_splitter: None,
        }
    }

    /// Create an extractor with all features enabled using defaults
    pub fn with_all_features() -> Self {
        Self {
            metadata_extractor: Some(MetadataExtractor::new()),
            schema_extractor: Some(SchemaExtractor::default()),
            selector_extractor: None,
            block_splitter: Some(BlockSplitter::default()),
        }
    }

    /// Enable metadata extraction
    pub fn with_metadata(mut self) -> Self {
        self.metadata_extractor = Some(MetadataExtractor::new());
        self
    }

    /// Enable schema extraction
    pub fn with_schema(mut self) -> Self {
        self.schema_extractor = Some(SchemaExtractor::default());
        self
    }

    /// Enable schema extraction with custom config
    pub fn with_schema_config(mut self, config: SchemaConfig) -> Self {
        self.schema_extractor = Some(SchemaExtractor::new(config));
        self
    }

    /// Enable custom selector extraction
    pub fn with_selectors(mut self, extractor: SelectorExtractor) -> Self {
        self.selector_extractor = Some(extractor);
        self
    }

    /// Enable block splitting
    pub fn with_blocks(mut self) -> Self {
        self.block_splitter = Some(BlockSplitter::default());
        self
    }

    /// Enable block splitting with custom config
    pub fn with_block_config(mut self, config: BlockConfig) -> Self {
        self.block_splitter = Some(BlockSplitter::new(config));
        self
    }

    /// Extract all enabled features from HTML (parses DOM once internally)
    pub fn extract(&self, html: &str) -> Result<ExtractionResult, ExtractorError> {
        let document = Html::parse_document(html);
        self.extract_from_dom(&document)
    }

    /// Extract all enabled features from a pre-parsed DOM, avoiding redundant parsing
    pub fn extract_from_dom(&self, document: &Html) -> Result<ExtractionResult, ExtractorError> {
        let mut result = ExtractionResult::default();

        if let Some(ref extractor) = self.metadata_extractor {
            result.metadata = Some(extractor.extract_from_dom(document)?);
        }

        if let Some(ref extractor) = self.schema_extractor {
            result.schema = Some(extractor.extract_from_dom(document)?);
        }

        if let Some(ref extractor) = self.selector_extractor {
            result.custom = Some(extractor.extract_from_dom(document)?);
        }

        if let Some(ref splitter) = self.block_splitter {
            result.blocks = Some(splitter.split_from_dom(document)?);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combined_extraction() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Test Page</title>
                <meta name="description" content="A test page">
                <meta property="og:title" content="OG Title">
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@type": "Article",
                    "headline": "Test Article"
                }
                </script>
            </head>
            <body>
                <h1>Main Title</h1>
                <p>Some paragraph content that is long enough to be meaningful.</p>

                <h2>Section</h2>
                <p>Section content with sufficient text to pass block extraction.</p>
            </body>
            </html>
        "#;

        let extractor = Extractor::with_all_features();
        let result = extractor.extract(html).unwrap();

        // Check metadata
        assert!(result.metadata.is_some());
        let metadata = result.metadata.unwrap();
        assert_eq!(metadata.title, Some("Test Page".to_string()));

        // Check schema
        assert!(result.schema.is_some());
        let schema = result.schema.unwrap();
        assert_eq!(schema.items.len(), 1);
        assert_eq!(schema.items[0].schema_type, "Article");

        // Check blocks
        assert!(result.blocks.is_some());
        let blocks = result.blocks.unwrap();
        assert!(blocks.count > 0);
    }

    #[test]
    fn test_selective_extraction() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head><title>Test</title></head>
            <body><p>Content</p></body>
            </html>
        "#;

        // Only metadata
        let extractor = Extractor::new().with_metadata();
        let result = extractor.extract(html).unwrap();

        assert!(result.metadata.is_some());
        assert!(result.schema.is_none());
        assert!(result.custom.is_none());
        assert!(result.blocks.is_none());
    }

    #[test]
    fn test_to_document_fields() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Test</title>
                <meta name="description" content="Description">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = Extractor::new().with_metadata();
        let result = extractor.extract(html).unwrap();
        let fields = result.to_document_fields();

        assert_eq!(
            fields.get("title"),
            Some(&Value::String("Test".to_string()))
        );
        assert_eq!(
            fields.get("description"),
            Some(&Value::String("Description".to_string()))
        );
    }
}
