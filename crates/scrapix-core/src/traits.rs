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

    /// Flush pending documents, returns number flushed
    async fn flush(&self) -> Result<usize>;

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
