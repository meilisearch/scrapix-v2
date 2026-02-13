//! # Scrapix Parser
//!
//! HTML parsing and content extraction.
//!
//! ## Features
//!
//! - DOM parsing with scraper
//! - Content extraction (readability-style algorithm)
//! - HTML to Markdown conversion
//! - Language detection
//! - Metadata extraction (Open Graph, JSON-LD)
//! - Link extraction
//!
//! ## Example
//!
//! ```rust,no_run
//! use scrapix_parser::{HtmlParser, HtmlParserBuilder};
//! use scrapix_core::RawPage;
//! use std::collections::HashMap;
//! use chrono::Utc;
//!
//! // Create a parser
//! let parser = HtmlParserBuilder::new()
//!     .extract_content(true)
//!     .convert_to_markdown(true)
//!     .detect_language(true)
//!     .build();
//!
//! // Parse a page
//! let page = RawPage {
//!     url: "https://example.com".to_string(),
//!     final_url: "https://example.com".to_string(),
//!     status: 200,
//!     headers: HashMap::new(),
//!     html: "<html><body><h1>Hello</h1><p>World</p></body></html>".to_string(),
//!     content_type: Some("text/html".to_string()),
//!     js_rendered: false,
//!     fetched_at: Utc::now(),
//!     fetch_duration_ms: 100,
//! };
//!
//! let doc = parser.parse(&page).unwrap();
//! println!("Title: {:?}", doc.title);
//! println!("Content: {:?}", doc.content);
//! ```

pub mod html;
pub mod language;
pub mod markdown;
pub mod readability;

// Re-exports for convenience
pub use html::{HtmlParser, HtmlParserBuilder, HtmlParserConfig};
pub use language::{
    detect_language, detect_language_info, detect_language_with_threshold, LanguageInfo,
};
pub use markdown::{
    html_to_main_content_markdown, html_to_markdown, html_to_markdown_with_config,
    markdown_to_text, MarkdownConfig,
};
pub use readability::{
    extract_content, extract_content_from_dom, extract_content_with_config, ReadabilityConfig,
};

// Re-export scraper::Html so callers that already have a parsed DOM can use it
pub use scraper::Html;
