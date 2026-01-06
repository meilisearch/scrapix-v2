//! Core traits for Scrapix components

use async_trait::async_trait;

use crate::{CrawlUrl, Document, RawPage, Result};

/// Trait for URL frontier management
#[async_trait]
pub trait Frontier: Send + Sync {
    /// Add a URL to the frontier
    async fn push(&self, url: CrawlUrl) -> Result<()>;

    /// Add multiple URLs to the frontier
    async fn push_many(&self, urls: Vec<CrawlUrl>) -> Result<()>;

    /// Get the next URL to crawl (respecting politeness)
    async fn pop(&self) -> Result<Option<CrawlUrl>>;

    /// Check if a URL has been seen
    async fn is_seen(&self, url: &str) -> Result<bool>;

    /// Mark a URL as seen
    async fn mark_seen(&self, url: &str) -> Result<()>;

    /// Get the number of pending URLs
    async fn pending_count(&self) -> Result<u64>;

    /// Get the number of seen URLs
    async fn seen_count(&self) -> Result<u64>;
}

/// Trait for page fetching
#[async_trait]
pub trait Fetcher: Send + Sync {
    /// Fetch a page by URL
    async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage>;

    /// Check if URL is allowed by robots.txt
    async fn is_allowed(&self, url: &str) -> Result<bool>;

    /// Get crawl delay for domain (from robots.txt)
    async fn get_crawl_delay(&self, domain: &str) -> Result<Option<u64>>;
}

/// Trait for HTML parsing
#[async_trait]
pub trait Parser: Send + Sync {
    /// Parse a raw page into a document
    async fn parse(&self, page: &RawPage) -> Result<Document>;

    /// Extract links from a page
    async fn extract_links(&self, page: &RawPage) -> Result<Vec<String>>;
}

/// Trait for feature extraction
#[async_trait]
pub trait FeatureExtractor: Send + Sync {
    /// Name of this feature
    fn name(&self) -> &'static str;

    /// Process a document and add extracted features
    async fn extract(&self, doc: &mut Document, page: &RawPage) -> Result<()>;

    /// Check if this feature should run for a given URL
    fn should_run(&self, url: &str) -> bool;
}

/// Trait for document storage/indexing
#[async_trait]
pub trait Storage: Send + Sync {
    /// Add a document to the index
    async fn add(&self, doc: Document) -> Result<()>;

    /// Add multiple documents to the index
    async fn add_batch(&self, docs: Vec<Document>) -> Result<()>;

    /// Flush pending documents
    async fn flush(&self) -> Result<()>;

    /// Get document count
    async fn count(&self) -> Result<u64>;
}

/// Trait for message queue operations
#[async_trait]
pub trait Queue: Send + Sync {
    type Message: Send;

    /// Send a message to the queue
    async fn send(&self, topic: &str, message: Self::Message) -> Result<()>;

    /// Send multiple messages to the queue
    async fn send_batch(&self, topic: &str, messages: Vec<Self::Message>) -> Result<()>;

    /// Receive messages from the queue
    async fn receive(&self, topic: &str, max_messages: usize) -> Result<Vec<Self::Message>>;

    /// Acknowledge message processing
    async fn ack(&self, topic: &str, message_id: &str) -> Result<()>;
}

/// Trait for metrics collection
pub trait Metrics: Send + Sync {
    /// Increment a counter
    fn increment(&self, name: &str, value: u64);

    /// Record a gauge value
    fn gauge(&self, name: &str, value: f64);

    /// Record a histogram value
    fn histogram(&self, name: &str, value: f64);

    /// Record timing in milliseconds
    fn timing(&self, name: &str, ms: u64);
}

/// Trait for webhook notifications
#[async_trait]
pub trait WebhookSender: Send + Sync {
    /// Send a webhook notification
    async fn send(&self, event: &str, payload: serde_json::Value) -> Result<()>;
}

/// Trait for rate limiting
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Wait until allowed to proceed for a domain
    async fn acquire(&self, domain: &str) -> Result<()>;

    /// Set rate limit for a domain
    async fn set_limit(&self, domain: &str, requests_per_second: f64) -> Result<()>;

    /// Get current rate for a domain
    async fn get_rate(&self, domain: &str) -> Result<f64>;
}

/// Trait for DNS resolution with caching
#[async_trait]
pub trait DnsResolver: Send + Sync {
    /// Resolve a hostname to IP addresses
    async fn resolve(&self, hostname: &str) -> Result<Vec<std::net::IpAddr>>;

    /// Clear the DNS cache
    async fn clear_cache(&self) -> Result<()>;

    /// Get cache hit rate
    fn cache_hit_rate(&self) -> f64;
}
