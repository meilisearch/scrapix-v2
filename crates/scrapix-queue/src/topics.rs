//! Topic definitions for the message queue

use serde::{Deserialize, Serialize};

use scrapix_core::{CrawlUrl, Document, UrlPatterns};

/// Predefined topic names
pub mod names {
    /// URLs to be crawled
    pub const URL_FRONTIER: &str = "scrapix.urls.frontier";
    /// URLs currently being processed
    pub const URL_PROCESSING: &str = "scrapix.urls.processing";
    /// Raw crawled pages awaiting content extraction
    pub const PAGES_RAW: &str = "scrapix.pages.raw";
    /// Processed documents ready for indexing
    pub const DOCUMENTS: &str = "scrapix.documents";
    /// Failed URLs (dead letter queue)
    pub const DLQ_URLS: &str = "scrapix.dlq.urls";
    /// Crawl events for monitoring
    pub const EVENTS: &str = "scrapix.events";
    /// Job status updates
    pub const JOB_STATUS: &str = "scrapix.jobs.status";
    /// Link graph updates (discovered links)
    pub const LINKS: &str = "scrapix.links";
    /// Crawl history updates (for incremental crawling)
    pub const CRAWL_HISTORY: &str = "scrapix.crawl.history";
}

/// Message types for the URL frontier queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlMessage {
    /// The URL to crawl
    pub url: CrawlUrl,
    /// Job ID this URL belongs to
    pub job_id: String,
    /// Index UID for the destination
    pub index_uid: String,
    /// Account ID for billing attribution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Message ID for tracking
    pub message_id: String,
    /// Timestamp when the message was created
    pub created_at: i64,
    /// URL patterns for filtering discovered URLs (optional, inherited from job config)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_patterns: Option<UrlPatterns>,
    /// Per-job Meilisearch URL (overrides global env var)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meilisearch_url: Option<String>,
    /// Per-job Meilisearch API key (overrides global env var)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meilisearch_api_key: Option<String>,
}

impl UrlMessage {
    pub fn new(url: CrawlUrl, job_id: impl Into<String>, index_uid: impl Into<String>) -> Self {
        Self {
            url,
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            account_id: None,
            message_id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
            url_patterns: None,
            meilisearch_url: None,
            meilisearch_api_key: None,
        }
    }

    /// Create a new URL message with account ID
    pub fn with_account(
        url: CrawlUrl,
        job_id: impl Into<String>,
        index_uid: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            url,
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            account_id: Some(account_id.into()),
            message_id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
            url_patterns: None,
            meilisearch_url: None,
            meilisearch_api_key: None,
        }
    }

    /// Create a new URL message with URL patterns
    pub fn with_patterns(
        url: CrawlUrl,
        job_id: impl Into<String>,
        index_uid: impl Into<String>,
        patterns: UrlPatterns,
    ) -> Self {
        Self {
            url,
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            account_id: None,
            message_id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
            url_patterns: Some(patterns),
            meilisearch_url: None,
            meilisearch_api_key: None,
        }
    }

    /// Set account ID (builder pattern)
    pub fn account(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = Some(account_id.into());
        self
    }

    /// Set per-job Meilisearch URL and API key (builder pattern)
    pub fn with_meilisearch(mut self, url: Option<String>, api_key: Option<String>) -> Self {
        self.meilisearch_url = url;
        self.meilisearch_api_key = api_key;
        self
    }

    /// Get the partition key (domain for locality)
    pub fn partition_key(&self) -> String {
        // Extract domain from URL for partitioning
        url::Url::parse(&self.url.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_else(|| self.job_id.clone())
    }
}

/// Message for raw crawled pages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPageMessage {
    /// Source URL
    pub url: String,
    /// Final URL after redirects
    pub final_url: String,
    /// HTTP status code
    pub status: u16,
    /// Raw HTML content
    pub html: String,
    /// Content type
    pub content_type: Option<String>,
    /// Content length in bytes (for billing)
    #[serde(default)]
    pub content_length: u64,
    /// Whether JS was rendered
    pub js_rendered: bool,
    /// Fetch timestamp (millis)
    pub fetched_at: i64,
    /// Fetch duration (millis)
    pub fetch_duration_ms: u64,
    /// Job ID
    pub job_id: String,
    /// Index UID
    pub index_uid: String,
    /// Account ID for billing attribution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Message ID
    pub message_id: String,
    /// ETag from response (for incremental crawling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Last-Modified from response (for incremental crawling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Per-job Meilisearch URL (overrides global env var)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meilisearch_url: Option<String>,
    /// Per-job Meilisearch API key (overrides global env var)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meilisearch_api_key: Option<String>,
}

/// Message for processed documents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMessage {
    /// The processed document
    pub document: Document,
    /// Job ID
    pub job_id: String,
    /// Index UID
    pub index_uid: String,
    /// Message ID
    pub message_id: String,
}

impl DocumentMessage {
    pub fn new(
        document: Document,
        job_id: impl Into<String>,
        index_uid: impl Into<String>,
    ) -> Self {
        Self {
            document,
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            message_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Crawl event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CrawlEvent {
    /// Job started
    JobStarted {
        job_id: String,
        index_uid: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        start_urls: Vec<String>,
        timestamp: i64,
    },
    /// Job completed
    JobCompleted {
        job_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        pages_crawled: u64,
        documents_indexed: u64,
        errors: u64,
        /// Total bytes downloaded during the job
        #[serde(default)]
        bytes_downloaded: u64,
        duration_secs: u64,
        timestamp: i64,
    },
    /// Job failed
    JobFailed {
        job_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        error: String,
        timestamp: i64,
    },
    /// Page crawled successfully
    PageCrawled {
        job_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        url: String,
        status: u16,
        /// Content length in bytes (for billing)
        #[serde(default)]
        content_length: u64,
        duration_ms: u64,
        timestamp: i64,
    },
    /// Page crawl failed
    PageFailed {
        job_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        url: String,
        error: String,
        retry_count: u32,
        timestamp: i64,
    },
    /// Document indexed
    DocumentIndexed {
        job_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        url: String,
        document_id: String,
        timestamp: i64,
    },
    /// URLs discovered
    UrlsDiscovered {
        job_id: String,
        source_url: String,
        count: usize,
        timestamp: i64,
    },
    /// Rate limited
    RateLimited {
        job_id: String,
        domain: String,
        wait_ms: u64,
        timestamp: i64,
    },
    /// Page skipped (duplicate, filtered, etc.)
    PageSkipped {
        job_id: String,
        url: String,
        reason: String,
        timestamp: i64,
    },
}

impl CrawlEvent {
    pub fn job_started(
        job_id: impl Into<String>,
        index_uid: impl Into<String>,
        start_urls: Vec<String>,
    ) -> Self {
        Self::JobStarted {
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            account_id: None,
            start_urls,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create job started event with account ID
    pub fn job_started_with_account(
        job_id: impl Into<String>,
        index_uid: impl Into<String>,
        account_id: impl Into<String>,
        start_urls: Vec<String>,
    ) -> Self {
        Self::JobStarted {
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            account_id: Some(account_id.into()),
            start_urls,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn page_crawled(
        job_id: impl Into<String>,
        url: impl Into<String>,
        status: u16,
        duration_ms: u64,
    ) -> Self {
        Self::PageCrawled {
            job_id: job_id.into(),
            account_id: None,
            url: url.into(),
            status,
            content_length: 0,
            duration_ms,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Create page crawled event with content length for billing
    pub fn page_crawled_with_billing(
        job_id: impl Into<String>,
        account_id: Option<String>,
        url: impl Into<String>,
        status: u16,
        content_length: u64,
        duration_ms: u64,
    ) -> Self {
        Self::PageCrawled {
            job_id: job_id.into(),
            account_id,
            url: url.into(),
            status,
            content_length,
            duration_ms,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn page_failed(
        job_id: impl Into<String>,
        url: impl Into<String>,
        error: impl Into<String>,
        retry_count: u32,
    ) -> Self {
        Self::PageFailed {
            job_id: job_id.into(),
            account_id: None,
            url: url.into(),
            error: error.into(),
            retry_count,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Dead letter queue message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlqMessage {
    /// Original message (JSON)
    pub original_message: String,
    /// Original topic
    pub original_topic: String,
    /// Error that caused the failure
    pub error: String,
    /// Number of retry attempts
    pub retry_count: u32,
    /// Timestamp of last failure
    pub failed_at: i64,
    /// Job ID if available
    pub job_id: Option<String>,
}

impl DlqMessage {
    pub fn new(
        original_message: impl Into<String>,
        original_topic: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            original_message: original_message.into(),
            original_topic: original_topic.into(),
            error: error.into(),
            retry_count: 1,
            failed_at: chrono::Utc::now().timestamp_millis(),
            job_id: None,
        }
    }

    pub fn with_job_id(mut self, job_id: impl Into<String>) -> Self {
        self.job_id = Some(job_id.into());
        self
    }

    pub fn increment_retry(mut self) -> Self {
        self.retry_count += 1;
        self.failed_at = chrono::Utc::now().timestamp_millis();
        self
    }
}

/// Message for link graph updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinksMessage {
    /// Source URL where links were found
    pub source_url: String,
    /// Target URLs (outbound links)
    pub target_urls: Vec<String>,
    /// Job ID
    pub job_id: String,
    /// Timestamp
    pub timestamp: i64,
}

impl LinksMessage {
    pub fn new(
        source_url: impl Into<String>,
        target_urls: Vec<String>,
        job_id: impl Into<String>,
    ) -> Self {
        Self {
            source_url: source_url.into(),
            target_urls,
            job_id: job_id.into(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Message for crawl history updates (for recrawl scheduling)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlHistoryMessage {
    /// URL that was crawled
    pub url: String,
    /// ETag from response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Last-Modified from response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// SHA-256 hash of content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// HTTP status code
    pub status: u16,
    /// Content length
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,
    /// Whether content changed since last crawl
    pub content_changed: bool,
    /// Job ID
    pub job_id: String,
    /// Timestamp
    pub timestamp: i64,
}

impl CrawlHistoryMessage {
    pub fn new(url: impl Into<String>, status: u16, job_id: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            etag: None,
            last_modified: None,
            content_hash: None,
            status,
            content_length: None,
            content_changed: true, // Assume changed by default
            job_id: job_id.into(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.etag = Some(etag.into());
        self
    }

    pub fn with_last_modified(mut self, last_modified: impl Into<String>) -> Self {
        self.last_modified = Some(last_modified.into());
        self
    }

    pub fn with_content_hash(mut self, hash: impl Into<String>) -> Self {
        self.content_hash = Some(hash.into());
        self
    }

    pub fn with_content_length(mut self, length: u64) -> Self {
        self.content_length = Some(length);
        self
    }

    pub fn with_content_changed(mut self, changed: bool) -> Self {
        self.content_changed = changed;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_links_message_creation() {
        let msg = LinksMessage::new(
            "https://example.com/page",
            vec![
                "https://example.com/link1".to_string(),
                "https://example.com/link2".to_string(),
            ],
            "job-123",
        );

        assert_eq!(msg.source_url, "https://example.com/page");
        assert_eq!(msg.target_urls.len(), 2);
        assert_eq!(msg.job_id, "job-123");
        assert!(msg.timestamp > 0);
    }

    #[test]
    fn test_links_message_serialization() {
        let msg = LinksMessage::new(
            "https://example.com/source",
            vec!["https://example.com/target".to_string()],
            "job-456",
        );

        let json = serde_json::to_string(&msg).expect("Failed to serialize");
        let deserialized: LinksMessage =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.source_url, msg.source_url);
        assert_eq!(deserialized.target_urls, msg.target_urls);
        assert_eq!(deserialized.job_id, msg.job_id);
    }

    #[test]
    fn test_crawl_history_message_creation() {
        let msg = CrawlHistoryMessage::new("https://example.com/page", 200, "job-789");

        assert_eq!(msg.url, "https://example.com/page");
        assert_eq!(msg.status, 200);
        assert_eq!(msg.job_id, "job-789");
        assert!(msg.content_changed); // Default is true
        assert!(msg.etag.is_none());
        assert!(msg.last_modified.is_none());
        assert!(msg.content_hash.is_none());
    }

    #[test]
    fn test_crawl_history_message_builder() {
        let msg = CrawlHistoryMessage::new("https://example.com/page", 200, "job-123")
            .with_etag("\"abc123\"")
            .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT")
            .with_content_hash("sha256:deadbeef")
            .with_content_length(12345)
            .with_content_changed(false);

        assert_eq!(msg.etag, Some("\"abc123\"".to_string()));
        assert_eq!(
            msg.last_modified,
            Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
        );
        assert_eq!(msg.content_hash, Some("sha256:deadbeef".to_string()));
        assert_eq!(msg.content_length, Some(12345));
        assert!(!msg.content_changed);
    }

    #[test]
    fn test_crawl_history_message_serialization() {
        let msg = CrawlHistoryMessage::new("https://example.com/page", 200, "job-123")
            .with_etag("\"etag\"")
            .with_content_hash("hash123");

        let json = serde_json::to_string(&msg).expect("Failed to serialize");
        let deserialized: CrawlHistoryMessage =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(deserialized.url, msg.url);
        assert_eq!(deserialized.status, msg.status);
        assert_eq!(deserialized.etag, msg.etag);
        assert_eq!(deserialized.content_hash, msg.content_hash);
    }

    #[test]
    fn test_url_message_partition_key() {
        let url = CrawlUrl::seed("https://example.com/path/to/page");
        let msg = UrlMessage::new(url, "job-1", "index-1");

        let key = msg.partition_key();
        assert_eq!(key, "example.com");
    }

    #[test]
    fn test_document_message_creation() {
        let doc = Document::new("https://example.com/doc", "example.com");
        let msg = DocumentMessage::new(doc.clone(), "job-1", "index-1");

        assert_eq!(msg.document.url, "https://example.com/doc");
        assert_eq!(msg.job_id, "job-1");
        assert_eq!(msg.index_uid, "index-1");
        assert!(!msg.message_id.is_empty());
    }

    #[test]
    fn test_dlq_message_creation() {
        let msg = DlqMessage::new(
            r#"{"url": "https://failed.com"}"#,
            "scrapix.urls.frontier",
            "Connection timeout",
        )
        .with_job_id("job-failed");

        assert!(msg.original_message.contains("failed.com"));
        assert_eq!(msg.original_topic, "scrapix.urls.frontier");
        assert_eq!(msg.error, "Connection timeout");
        assert_eq!(msg.job_id, Some("job-failed".to_string()));
        assert_eq!(msg.retry_count, 1);

        // Test increment_retry
        let msg = msg.increment_retry();
        assert_eq!(msg.retry_count, 2);
    }

    #[test]
    fn test_crawl_event_constructors() {
        let started = CrawlEvent::job_started("job-1", "index-1", vec!["https://example.com".to_string()]);
        match started {
            CrawlEvent::JobStarted {
                job_id,
                index_uid,
                start_urls,
                ..
            } => {
                assert_eq!(job_id, "job-1");
                assert_eq!(index_uid, "index-1");
                assert_eq!(start_urls.len(), 1);
            }
            _ => panic!("Expected JobStarted"),
        }

        let crawled = CrawlEvent::page_crawled("job-1", "https://example.com", 200, 150);
        match crawled {
            CrawlEvent::PageCrawled {
                job_id,
                url,
                status,
                duration_ms,
                ..
            } => {
                assert_eq!(job_id, "job-1");
                assert_eq!(url, "https://example.com");
                assert_eq!(status, 200);
                assert_eq!(duration_ms, 150);
            }
            _ => panic!("Expected PageCrawled"),
        }

        let failed = CrawlEvent::page_failed("job-1", "https://failed.com", "Timeout", 3);
        match failed {
            CrawlEvent::PageFailed {
                job_id,
                url,
                error,
                retry_count,
                ..
            } => {
                assert_eq!(job_id, "job-1");
                assert_eq!(url, "https://failed.com");
                assert_eq!(error, "Timeout");
                assert_eq!(retry_count, 3);
            }
            _ => panic!("Expected PageFailed"),
        }
    }
}
