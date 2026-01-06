//! Metadata extraction from HTML documents
//!
//! Extracts meta tags, Open Graph tags, Twitter cards, and other metadata.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, instrument, warn};

/// Errors that can occur during metadata extraction
#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("Invalid selector: {0}")]
    InvalidSelector(String),

    #[error("Extraction error: {0}")]
    ExtractionError(String),
}

/// Extracted metadata from an HTML document
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedMetadata {
    /// Basic meta tags (name -> content)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub meta: HashMap<String, String>,

    /// Open Graph tags (og:* -> value)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub open_graph: HashMap<String, String>,

    /// Twitter Card tags (twitter:* -> value)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub twitter: HashMap<String, String>,

    /// Dublin Core metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub dublin_core: HashMap<String, String>,

    /// Canonical URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,

    /// Alternate URLs (hreflang -> url)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub alternate_urls: HashMap<String, String>,

    /// Favicon URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,

    /// RSS/Atom feed URLs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feeds: Vec<FeedLink>,

    /// Page title from <title> tag
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Robots directives
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robots: Option<String>,

    /// Author
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    /// Keywords
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Published date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,

    /// Modified date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_date: Option<String>,
}

/// RSS/Atom feed link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedLink {
    pub url: String,
    pub feed_type: String, // "rss" or "atom"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Metadata extractor
pub struct MetadataExtractor {
    // Precompiled selectors for performance
    meta_selector: Selector,
    title_selector: Selector,
    link_selector: Selector,
}

impl Default for MetadataExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataExtractor {
    /// Create a new metadata extractor
    pub fn new() -> Self {
        Self {
            meta_selector: Selector::parse("meta").expect("valid meta selector"),
            title_selector: Selector::parse("title").expect("valid title selector"),
            link_selector: Selector::parse("link").expect("valid link selector"),
        }
    }

    /// Extract all metadata from an HTML document
    #[instrument(skip(self, html), level = "debug")]
    pub fn extract(&self, html: &str) -> Result<ExtractedMetadata, MetadataError> {
        let document = Html::parse_document(html);
        let mut metadata = ExtractedMetadata::default();

        // Extract title
        if let Some(title_elem) = document.select(&self.title_selector).next() {
            let title = title_elem.text().collect::<String>().trim().to_string();
            if !title.is_empty() {
                metadata.title = Some(title);
            }
        }

        // Extract meta tags
        self.extract_meta_tags(&document, &mut metadata);

        // Extract link tags
        self.extract_link_tags(&document, &mut metadata);

        // Post-process: extract common fields from meta
        self.post_process(&mut metadata);

        debug!(
            meta_count = metadata.meta.len(),
            og_count = metadata.open_graph.len(),
            twitter_count = metadata.twitter.len(),
            "Extracted metadata"
        );

        Ok(metadata)
    }

    /// Extract meta tags
    fn extract_meta_tags(&self, document: &Html, metadata: &mut ExtractedMetadata) {
        for element in document.select(&self.meta_selector) {
            let name = element
                .value()
                .attr("name")
                .or_else(|| element.value().attr("property"))
                .or_else(|| element.value().attr("itemprop"));

            let content = element.value().attr("content");

            if let (Some(name), Some(content)) = (name, content) {
                let name_lower = name.to_lowercase();
                let content = content.trim().to_string();

                if content.is_empty() {
                    continue;
                }

                // Categorize by prefix
                if name_lower.starts_with("og:") {
                    let key = name_lower.strip_prefix("og:").unwrap().to_string();
                    metadata.open_graph.insert(key, content);
                } else if name_lower.starts_with("twitter:") {
                    let key = name_lower.strip_prefix("twitter:").unwrap().to_string();
                    metadata.twitter.insert(key, content);
                } else if name_lower.starts_with("dc.")
                    || name_lower.starts_with("dc:")
                    || name_lower.starts_with("dcterms.")
                {
                    // Dublin Core
                    let key = name_lower
                        .replace("dc.", "")
                        .replace("dc:", "")
                        .replace("dcterms.", "");
                    metadata.dublin_core.insert(key, content);
                } else if name_lower.starts_with("article:") {
                    // Article metadata (Facebook)
                    metadata.meta.insert(name_lower, content);
                } else {
                    // General meta tags
                    metadata.meta.insert(name_lower, content);
                }
            }

            // Handle charset
            if let Some(charset) = element.value().attr("charset") {
                metadata
                    .meta
                    .insert("charset".to_string(), charset.to_string());
            }

            // Handle http-equiv
            if let Some(http_equiv) = element.value().attr("http-equiv") {
                if let Some(content) = element.value().attr("content") {
                    metadata.meta.insert(
                        format!("http-equiv:{}", http_equiv.to_lowercase()),
                        content.to_string(),
                    );
                }
            }
        }
    }

    /// Extract link tags
    fn extract_link_tags(&self, document: &Html, metadata: &mut ExtractedMetadata) {
        for element in document.select(&self.link_selector) {
            let rel = element.value().attr("rel");
            let href = element.value().attr("href");

            if let (Some(rel), Some(href)) = (rel, href) {
                let rel_lower = rel.to_lowercase();
                let href = href.trim().to_string();

                if href.is_empty() {
                    continue;
                }

                match rel_lower.as_str() {
                    "canonical" => {
                        metadata.canonical_url = Some(href);
                    }
                    "icon" | "shortcut icon" | "apple-touch-icon" => {
                        // Prefer larger icons, but take first if none set
                        if metadata.favicon.is_none() {
                            metadata.favicon = Some(href);
                        }
                    }
                    "alternate" => {
                        // Check for RSS/Atom feeds
                        if let Some(feed_type) = element.value().attr("type") {
                            let feed_type_lower = feed_type.to_lowercase();
                            if feed_type_lower.contains("rss") {
                                metadata.feeds.push(FeedLink {
                                    url: href.clone(),
                                    feed_type: "rss".to_string(),
                                    title: element.value().attr("title").map(|s| s.to_string()),
                                });
                            } else if feed_type_lower.contains("atom") {
                                metadata.feeds.push(FeedLink {
                                    url: href.clone(),
                                    feed_type: "atom".to_string(),
                                    title: element.value().attr("title").map(|s| s.to_string()),
                                });
                            }
                        }

                        // Check for hreflang alternates
                        if let Some(hreflang) = element.value().attr("hreflang") {
                            metadata
                                .alternate_urls
                                .insert(hreflang.to_string(), href.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Post-process extracted metadata to populate common fields
    fn post_process(&self, metadata: &mut ExtractedMetadata) {
        // Description: prefer og:description > twitter:description > meta description
        if metadata.description.is_none() {
            metadata.description = metadata
                .open_graph
                .get("description")
                .cloned()
                .or_else(|| metadata.twitter.get("description").cloned())
                .or_else(|| metadata.meta.get("description").cloned());
        }

        // Author
        if metadata.author.is_none() {
            metadata.author = metadata
                .meta
                .get("author")
                .cloned()
                .or_else(|| metadata.dublin_core.get("creator").cloned())
                .or_else(|| metadata.meta.get("article:author").cloned());
        }

        // Robots
        if metadata.robots.is_none() {
            metadata.robots = metadata.meta.get("robots").cloned();
        }

        // Keywords
        if metadata.keywords.is_empty() {
            if let Some(keywords_str) = metadata.meta.get("keywords") {
                metadata.keywords = keywords_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        // Published date: check various sources
        if metadata.published_date.is_none() {
            metadata.published_date = metadata
                .meta
                .get("article:published_time")
                .cloned()
                .or_else(|| metadata.dublin_core.get("date").cloned())
                .or_else(|| metadata.meta.get("date").cloned())
                .or_else(|| metadata.meta.get("pubdate").cloned())
                .or_else(|| metadata.meta.get("datePublished").cloned());
        }

        // Modified date
        if metadata.modified_date.is_none() {
            metadata.modified_date = metadata
                .meta
                .get("article:modified_time")
                .cloned()
                .or_else(|| metadata.meta.get("lastmod").cloned())
                .or_else(|| metadata.meta.get("dateModified").cloned());
        }

        // Title fallback: og:title > twitter:title
        if metadata.title.is_none() {
            metadata.title = metadata
                .open_graph
                .get("title")
                .cloned()
                .or_else(|| metadata.twitter.get("title").cloned());
        }
    }

    /// Convert extracted metadata to a flat HashMap suitable for document storage
    pub fn to_flat_map(&self, metadata: &ExtractedMetadata) -> HashMap<String, String> {
        let mut map = HashMap::new();

        // Add basic meta tags
        for (key, value) in &metadata.meta {
            map.insert(format!("meta:{}", key), value.clone());
        }

        // Add Open Graph tags
        for (key, value) in &metadata.open_graph {
            map.insert(format!("og:{}", key), value.clone());
        }

        // Add Twitter tags
        for (key, value) in &metadata.twitter {
            map.insert(format!("twitter:{}", key), value.clone());
        }

        // Add Dublin Core
        for (key, value) in &metadata.dublin_core {
            map.insert(format!("dc:{}", key), value.clone());
        }

        // Add top-level fields
        if let Some(ref url) = metadata.canonical_url {
            map.insert("canonical_url".to_string(), url.clone());
        }
        if let Some(ref title) = metadata.title {
            map.insert("title".to_string(), title.clone());
        }
        if let Some(ref description) = metadata.description {
            map.insert("description".to_string(), description.clone());
        }
        if let Some(ref author) = metadata.author {
            map.insert("author".to_string(), author.clone());
        }
        if let Some(ref robots) = metadata.robots {
            map.insert("robots".to_string(), robots.clone());
        }
        if let Some(ref published) = metadata.published_date {
            map.insert("published_date".to_string(), published.clone());
        }
        if let Some(ref modified) = metadata.modified_date {
            map.insert("modified_date".to_string(), modified.clone());
        }
        if !metadata.keywords.is_empty() {
            map.insert("keywords".to_string(), metadata.keywords.join(", "));
        }

        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_basic_meta() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Test Page</title>
                <meta name="description" content="A test page">
                <meta name="keywords" content="test, page, example">
                <meta name="author" content="John Doe">
                <meta name="robots" content="index, follow">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = MetadataExtractor::new();
        let metadata = extractor.extract(html).unwrap();

        assert_eq!(metadata.title, Some("Test Page".to_string()));
        assert_eq!(metadata.description, Some("A test page".to_string()));
        assert_eq!(metadata.author, Some("John Doe".to_string()));
        assert_eq!(metadata.robots, Some("index, follow".to_string()));
        assert_eq!(metadata.keywords, vec!["test", "page", "example"]);
    }

    #[test]
    fn test_extract_open_graph() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta property="og:title" content="OG Title">
                <meta property="og:description" content="OG Description">
                <meta property="og:image" content="https://example.com/image.jpg">
                <meta property="og:url" content="https://example.com/page">
                <meta property="og:type" content="article">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = MetadataExtractor::new();
        let metadata = extractor.extract(html).unwrap();

        assert_eq!(
            metadata.open_graph.get("title"),
            Some(&"OG Title".to_string())
        );
        assert_eq!(
            metadata.open_graph.get("description"),
            Some(&"OG Description".to_string())
        );
        assert_eq!(
            metadata.open_graph.get("image"),
            Some(&"https://example.com/image.jpg".to_string())
        );
        // Description should fall back to OG
        assert_eq!(metadata.description, Some("OG Description".to_string()));
    }

    #[test]
    fn test_extract_twitter_cards() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta name="twitter:card" content="summary_large_image">
                <meta name="twitter:title" content="Twitter Title">
                <meta name="twitter:description" content="Twitter Description">
                <meta name="twitter:image" content="https://example.com/twitter.jpg">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = MetadataExtractor::new();
        let metadata = extractor.extract(html).unwrap();

        assert_eq!(
            metadata.twitter.get("card"),
            Some(&"summary_large_image".to_string())
        );
        assert_eq!(
            metadata.twitter.get("title"),
            Some(&"Twitter Title".to_string())
        );
    }

    #[test]
    fn test_extract_canonical_and_links() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <link rel="canonical" href="https://example.com/canonical">
                <link rel="icon" href="/favicon.ico">
                <link rel="alternate" type="application/rss+xml" title="RSS Feed" href="/feed.xml">
                <link rel="alternate" hreflang="es" href="https://example.com/es/page">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = MetadataExtractor::new();
        let metadata = extractor.extract(html).unwrap();

        assert_eq!(
            metadata.canonical_url,
            Some("https://example.com/canonical".to_string())
        );
        assert_eq!(metadata.favicon, Some("/favicon.ico".to_string()));
        assert_eq!(metadata.feeds.len(), 1);
        assert_eq!(metadata.feeds[0].feed_type, "rss");
        assert_eq!(
            metadata.alternate_urls.get("es"),
            Some(&"https://example.com/es/page".to_string())
        );
    }

    #[test]
    fn test_extract_article_dates() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta property="article:published_time" content="2024-01-15T10:00:00Z">
                <meta property="article:modified_time" content="2024-01-16T15:30:00Z">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = MetadataExtractor::new();
        let metadata = extractor.extract(html).unwrap();

        assert_eq!(
            metadata.published_date,
            Some("2024-01-15T10:00:00Z".to_string())
        );
        assert_eq!(
            metadata.modified_date,
            Some("2024-01-16T15:30:00Z".to_string())
        );
    }

    #[test]
    fn test_to_flat_map() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Test</title>
                <meta name="description" content="A test">
                <meta property="og:title" content="OG Test">
            </head>
            <body></body>
            </html>
        "#;

        let extractor = MetadataExtractor::new();
        let metadata = extractor.extract(html).unwrap();
        let flat = extractor.to_flat_map(&metadata);

        assert!(flat.contains_key("title"));
        assert!(flat.contains_key("description"));
        assert!(flat.contains_key("og:title"));
    }
}
