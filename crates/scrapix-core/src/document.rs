//! Document types for Scrapix

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A crawled and processed document ready for indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Unique document identifier
    pub uid: String,

    /// Source URL
    pub url: String,

    /// Domain/hostname
    pub domain: String,

    /// Page title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// URL path segments for hierarchical filtering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urls_tags: Option<Vec<String>>,

    /// Main content (cleaned text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Content as Markdown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,

    /// Meta tags extracted from the page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,

    /// Schema.org/JSON-LD data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,

    /// Custom selector extractions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<HashMap<String, serde_json::Value>>,

    /// AI extraction results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_extraction: Option<serde_json::Value>,

    /// AI-generated summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_summary: Option<String>,

    /// Vector embeddings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeddings: Option<Vec<f32>>,

    /// Language code (ISO 639-1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Crawl timestamp
    pub crawled_at: DateTime<Utc>,

    /// Parent document ID (for block documents)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_document_id: Option<String>,

    /// Block index within parent document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_block: Option<u32>,

    /// Heading hierarchy for this block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h2: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h3: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h4: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h5: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h6: Option<String>,

    /// Anchor/fragment for this block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
}

impl Document {
    /// Create a new document with required fields
    pub fn new(url: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            uid: Uuid::new_v4().to_string(),
            url: url.into(),
            domain: domain.into(),
            title: None,
            urls_tags: None,
            content: None,
            markdown: None,
            metadata: None,
            schema: None,
            custom: None,
            ai_extraction: None,
            ai_summary: None,
            embeddings: None,
            language: None,
            crawled_at: Utc::now(),
            parent_document_id: None,
            page_block: None,
            h1: None,
            h2: None,
            h3: None,
            h4: None,
            h5: None,
            h6: None,
            anchor: None,
        }
    }

    /// Create a block document from a parent
    pub fn new_block(parent: &Document, block_index: u32) -> Self {
        Self {
            uid: Uuid::new_v4().to_string(),
            url: parent.url.clone(),
            domain: parent.domain.clone(),
            title: parent.title.clone(),
            urls_tags: parent.urls_tags.clone(),
            content: None,
            markdown: None,
            metadata: parent.metadata.clone(),
            schema: None,
            custom: None,
            ai_extraction: None,
            ai_summary: None,
            embeddings: None,
            language: parent.language.clone(),
            crawled_at: parent.crawled_at,
            parent_document_id: Some(parent.uid.clone()),
            page_block: Some(block_index),
            h1: None,
            h2: None,
            h3: None,
            h4: None,
            h5: None,
            h6: None,
            anchor: None,
        }
    }

    /// Check if this is a block document
    pub fn is_block(&self) -> bool {
        self.parent_document_id.is_some()
    }
}

/// Raw crawled page before processing
#[derive(Debug, Clone)]
pub struct RawPage {
    /// Source URL
    pub url: String,

    /// Final URL after redirects
    pub final_url: String,

    /// HTTP status code
    pub status: u16,

    /// Response headers
    pub headers: HashMap<String, String>,

    /// Raw HTML content
    pub html: String,

    /// Content type
    pub content_type: Option<String>,

    /// Whether JavaScript was rendered
    pub js_rendered: bool,

    /// Fetch timestamp
    pub fetched_at: DateTime<Utc>,

    /// Fetch duration in milliseconds
    pub fetch_duration_ms: u64,
}

/// URL to be crawled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlUrl {
    /// URL to crawl
    pub url: String,

    /// Crawl depth from seed URL
    pub depth: u32,

    /// Priority (higher = more important)
    pub priority: i32,

    /// Parent URL that linked to this
    pub parent_url: Option<String>,

    /// Anchor text from parent link
    pub anchor_text: Option<String>,

    /// Discovery timestamp
    pub discovered_at: DateTime<Utc>,

    /// Number of retry attempts
    pub retry_count: u32,

    /// Whether this requires JS rendering
    pub requires_js: bool,
}

impl CrawlUrl {
    pub fn new(url: impl Into<String>, depth: u32) -> Self {
        Self {
            url: url.into(),
            depth,
            priority: 0,
            parent_url: None,
            anchor_text: None,
            discovered_at: Utc::now(),
            retry_count: 0,
            requires_js: false,
        }
    }

    pub fn seed(url: impl Into<String>) -> Self {
        Self::new(url, 0)
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_parent(mut self, parent_url: impl Into<String>) -> Self {
        self.parent_url = Some(parent_url.into());
        self
    }
}

/// Crawl job status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

/// Crawl job state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobState {
    /// Job ID
    pub job_id: String,

    /// Current status
    pub status: JobStatus,

    /// Index UID being populated
    pub index_uid: String,

    /// Pages crawled
    pub pages_crawled: u64,

    /// Pages indexed
    pub pages_indexed: u64,

    /// Documents sent to Meilisearch
    pub documents_sent: u64,

    /// Errors encountered
    pub errors: u64,

    /// Start timestamp
    pub started_at: Option<DateTime<Utc>>,

    /// Completion timestamp
    pub completed_at: Option<DateTime<Utc>>,

    /// Error message if failed
    pub error_message: Option<String>,

    /// Current crawl rate (pages/minute)
    pub crawl_rate: f64,

    /// Estimated time remaining (seconds)
    pub eta_seconds: Option<u64>,
}

impl JobState {
    pub fn new(job_id: impl Into<String>, index_uid: impl Into<String>) -> Self {
        Self {
            job_id: job_id.into(),
            status: JobStatus::Pending,
            index_uid: index_uid.into(),
            pages_crawled: 0,
            pages_indexed: 0,
            documents_sent: 0,
            errors: 0,
            started_at: None,
            completed_at: None,
            error_message: None,
            crawl_rate: 0.0,
            eta_seconds: None,
        }
    }

    pub fn start(&mut self) {
        self.status = JobStatus::Running;
        self.started_at = Some(Utc::now());
    }

    pub fn complete(&mut self) {
        self.status = JobStatus::Completed;
        self.completed_at = Some(Utc::now());
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = JobStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error_message = Some(error.into());
    }

    pub fn duration_seconds(&self) -> Option<i64> {
        match (self.started_at, self.completed_at.or(Some(Utc::now()))) {
            (Some(start), Some(end)) => Some((end - start).num_seconds()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_creation() {
        let doc = Document::new("https://example.com/page", "example.com");
        assert!(!doc.uid.is_empty());
        assert_eq!(doc.url, "https://example.com/page");
        assert_eq!(doc.domain, "example.com");
        assert!(!doc.is_block());
    }

    #[test]
    fn test_block_document() {
        let parent = Document::new("https://example.com/page", "example.com");
        let block = Document::new_block(&parent, 0);

        assert!(block.is_block());
        assert_eq!(block.parent_document_id, Some(parent.uid.clone()));
        assert_eq!(block.page_block, Some(0));
    }

    #[test]
    fn test_crawl_url() {
        let url = CrawlUrl::seed("https://example.com")
            .with_priority(10)
            .with_parent("https://other.com");

        assert_eq!(url.depth, 0);
        assert_eq!(url.priority, 10);
        assert_eq!(url.parent_url, Some("https://other.com".to_string()));
    }
}
