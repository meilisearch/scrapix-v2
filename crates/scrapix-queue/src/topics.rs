//! Topic definitions for the message queue

use serde::{Deserialize, Serialize};

use scrapix_core::{CrawlUrl, Document};

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
    /// Message ID for tracking
    pub message_id: String,
    /// Timestamp when the message was created
    pub created_at: i64,
}

impl UrlMessage {
    pub fn new(url: CrawlUrl, job_id: impl Into<String>, index_uid: impl Into<String>) -> Self {
        Self {
            url,
            job_id: job_id.into(),
            index_uid: index_uid.into(),
            message_id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now().timestamp_millis(),
        }
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
    /// Message ID
    pub message_id: String,
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
        start_urls: Vec<String>,
        timestamp: i64,
    },
    /// Job completed
    JobCompleted {
        job_id: String,
        pages_crawled: u64,
        documents_indexed: u64,
        errors: u64,
        duration_secs: u64,
        timestamp: i64,
    },
    /// Job failed
    JobFailed {
        job_id: String,
        error: String,
        timestamp: i64,
    },
    /// Page crawled successfully
    PageCrawled {
        job_id: String,
        url: String,
        status: u16,
        duration_ms: u64,
        timestamp: i64,
    },
    /// Page crawl failed
    PageFailed {
        job_id: String,
        url: String,
        error: String,
        retry_count: u32,
        timestamp: i64,
    },
    /// Document indexed
    DocumentIndexed {
        job_id: String,
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
            url: url.into(),
            status,
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
