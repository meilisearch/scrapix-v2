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

use scrapix_core::{Document, Result, ScrapixError};
use url::Url;

/// Parse a server-provided markdown page into a Document.
///
/// Used when the server responds with `Content-Type: text/markdown`
/// (e.g. Cloudflare's "Markdown for Agents" feature). The markdown is used
/// directly instead of running our HTML→markdown conversion pipeline.
pub fn parse_markdown_page(url: &str, markdown: &str) -> Result<Document> {
    let parsed_url = Url::parse(url)?;
    let domain = parsed_url
        .host_str()
        .ok_or_else(|| ScrapixError::Parse("URL has no host".to_string()))?;

    let mut doc = Document::new(url, domain);

    // Extract title from first `# ` heading
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            let title = title.trim();
            if !title.is_empty() {
                doc.title = Some(title.to_string());
                break;
            }
        }
    }

    // Set markdown directly from server response
    doc.markdown = Some(markdown.to_string());

    // Derive plain text content from markdown
    let content = markdown_to_text(markdown);
    if !content.is_empty() {
        doc.content = Some(content.clone());

        // Detect language from content
        doc.language = detect_language(&content);
    }

    // Extract URL tags (path segments)
    let path = parsed_url.path();
    let mut tags = Vec::new();
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        tags.push(format!("/{}", current));
    }
    doc.urls_tags = Some(tags);

    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_page_basic() {
        let markdown = "# Hello World\n\nThis is a test page with some content.\n\n## Section\n\nMore content here.";
        let doc = parse_markdown_page("https://example.com/docs/test", markdown).unwrap();

        assert_eq!(doc.title, Some("Hello World".to_string()));
        assert_eq!(doc.domain, "example.com");
        assert_eq!(doc.markdown, Some(markdown.to_string()));
        assert!(doc.content.is_some());
        assert!(doc.content.as_ref().unwrap().contains("Hello World"));
        assert_eq!(doc.urls_tags, Some(vec!["/docs".to_string(), "/docs/test".to_string()]));
    }

    #[test]
    fn test_parse_markdown_page_no_heading() {
        let markdown = "Just some plain text content without a heading.";
        let doc = parse_markdown_page("https://example.com/", markdown).unwrap();

        assert_eq!(doc.title, None);
        assert!(doc.content.is_some());
    }

    #[test]
    fn test_parse_markdown_page_language_detection() {
        let markdown = "# English Page\n\nThis is a page written entirely in the English language with enough content for detection to work properly and accurately.";
        let doc = parse_markdown_page("https://example.com/page", markdown).unwrap();

        assert_eq!(doc.language, Some("en".to_string()));
    }
}
