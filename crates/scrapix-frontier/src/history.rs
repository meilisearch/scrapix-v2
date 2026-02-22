//! URL crawl history tracking for incremental crawling
//!
//! This module provides functionality to track:
//! - When URLs were last crawled
//! - HTTP caching headers (ETag, Last-Modified)
//! - Content fingerprints for change detection
//! - Change frequency for adaptive re-crawl scheduling
//!
//! ## Example
//!
//! ```rust,no_run
//! use scrapix_frontier::{UrlHistory, UrlHistoryConfig, CrawlRecord};
//! use std::time::Duration;
//!
//! // Create history tracker
//! let history = UrlHistory::new(UrlHistoryConfig::default());
//!
//! // Record a crawl
//! let record = CrawlRecord::new()
//!     .with_etag("abc123")
//!     .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT")
//!     .with_content_hash("sha256:abcdef...");
//!
//! history.record_crawl("https://example.com/page", record);
//!
//! // Check if re-crawl is needed
//! if let Some(_record) = history.get_record("https://example.com/page") {
//!     if history.should_recrawl("https://example.com/page") {
//!         // Re-crawl the page
//!     }
//! }
//! ```

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

/// Configuration for URL history tracking
#[derive(Debug, Clone)]
pub struct UrlHistoryConfig {
    /// Maximum number of URLs to track in memory
    pub max_entries: usize,
    /// Default re-crawl interval
    pub default_recrawl_interval: Duration,
    /// Minimum re-crawl interval (won't re-crawl faster than this)
    pub min_recrawl_interval: Duration,
    /// Maximum re-crawl interval (won't wait longer than this)
    pub max_recrawl_interval: Duration,
    /// Adaptive interval multiplier for unchanged content
    pub unchanged_multiplier: f64,
    /// Adaptive interval multiplier for changed content (< 1.0 to decrease interval)
    pub changed_multiplier: f64,
    /// Enable content fingerprinting
    pub enable_fingerprinting: bool,
}

impl Default for UrlHistoryConfig {
    fn default() -> Self {
        Self {
            max_entries: 1_000_000,
            default_recrawl_interval: Duration::from_secs(24 * 3600), // 24 hours
            min_recrawl_interval: Duration::from_secs(3600),          // 1 hour
            max_recrawl_interval: Duration::from_secs(7 * 24 * 3600), // 7 days
            unchanged_multiplier: 1.5,
            changed_multiplier: 0.5,
            enable_fingerprinting: true,
        }
    }
}

/// Record of a URL's crawl history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlRecord {
    /// When this URL was last crawled
    pub last_crawled_at: DateTime<Utc>,

    /// HTTP ETag header from last response
    pub etag: Option<String>,

    /// HTTP Last-Modified header from last response
    pub last_modified: Option<String>,

    /// SHA-256 hash of the content
    pub content_hash: Option<String>,

    /// Number of times this URL has been crawled
    pub crawl_count: u32,

    /// Number of times content changed between crawls
    pub change_count: u32,

    /// Current adaptive re-crawl interval in seconds
    pub recrawl_interval_secs: u64,

    /// HTTP status from last crawl
    pub last_status: Option<u16>,

    /// Content length from last crawl
    pub last_content_length: Option<u64>,
}

impl Default for CrawlRecord {
    fn default() -> Self {
        Self::new()
    }
}

impl CrawlRecord {
    /// Create a new crawl record
    pub fn new() -> Self {
        Self {
            last_crawled_at: Utc::now(),
            etag: None,
            last_modified: None,
            content_hash: None,
            crawl_count: 1,
            change_count: 0,
            recrawl_interval_secs: 24 * 3600, // 24 hours default
            last_status: None,
            last_content_length: None,
        }
    }

    /// Set the ETag
    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.etag = Some(etag.into());
        self
    }

    /// Set the Last-Modified header
    pub fn with_last_modified(mut self, last_modified: impl Into<String>) -> Self {
        self.last_modified = Some(last_modified.into());
        self
    }

    /// Set the content hash
    pub fn with_content_hash(mut self, hash: impl Into<String>) -> Self {
        self.content_hash = Some(hash.into());
        self
    }

    /// Set the HTTP status
    pub fn with_status(mut self, status: u16) -> Self {
        self.last_status = Some(status);
        self
    }

    /// Set the content length
    pub fn with_content_length(mut self, length: u64) -> Self {
        self.last_content_length = Some(length);
        self
    }

    /// Calculate content change rate (changes / crawls)
    pub fn change_rate(&self) -> f64 {
        if self.crawl_count == 0 {
            0.0
        } else {
            self.change_count as f64 / self.crawl_count as f64
        }
    }

    /// Get the re-crawl interval as Duration
    pub fn recrawl_interval(&self) -> Duration {
        Duration::from_secs(self.recrawl_interval_secs)
    }

    /// Check if the record has caching headers that can be used for conditional requests
    pub fn has_caching_headers(&self) -> bool {
        self.etag.is_some() || self.last_modified.is_some()
    }
}

/// In-memory URL history tracker
pub struct UrlHistory {
    config: UrlHistoryConfig,
    records: RwLock<HashMap<String, CrawlRecord>>,
}

impl UrlHistory {
    /// Create a new URL history tracker
    pub fn new(config: UrlHistoryConfig) -> Self {
        Self {
            config,
            records: RwLock::new(HashMap::new()),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(UrlHistoryConfig::default())
    }

    /// Get the crawl record for a URL
    pub fn get_record(&self, url: &str) -> Option<CrawlRecord> {
        let key = normalize_url(url);
        self.records.read().get(&key).cloned()
    }

    /// Record a new crawl of a URL
    pub fn record_crawl(&self, url: &str, mut record: CrawlRecord) {
        let key = normalize_url(url);
        let mut records = self.records.write();

        // Check if we need to evict old entries
        if records.len() >= self.config.max_entries && !records.contains_key(&key) {
            // Simple eviction: remove oldest entry
            // In production, use LRU or similar
            if let Some(oldest_key) = records
                .iter()
                .min_by_key(|(_, r)| r.last_crawled_at)
                .map(|(k, _)| k.clone())
            {
                records.remove(&oldest_key);
            }
        }

        // Update or insert record
        if let Some(existing) = records.get_mut(&key) {
            // Update existing record
            let content_changed = record.content_hash.as_ref() != existing.content_hash.as_ref();

            existing.last_crawled_at = record.last_crawled_at;
            existing.etag = record.etag.clone();
            existing.last_modified = record.last_modified.clone();
            existing.content_hash = record.content_hash.clone();
            existing.crawl_count += 1;
            existing.last_status = record.last_status;
            existing.last_content_length = record.last_content_length;

            if content_changed {
                existing.change_count += 1;
                // Decrease interval when content changes
                let new_interval =
                    (existing.recrawl_interval_secs as f64 * self.config.changed_multiplier) as u64;
                existing.recrawl_interval_secs =
                    new_interval.max(self.config.min_recrawl_interval.as_secs());
            } else {
                // Increase interval when content stays the same
                let new_interval = (existing.recrawl_interval_secs as f64
                    * self.config.unchanged_multiplier) as u64;
                existing.recrawl_interval_secs =
                    new_interval.min(self.config.max_recrawl_interval.as_secs());
            }
        } else {
            // New record
            record.recrawl_interval_secs = self.config.default_recrawl_interval.as_secs();
            records.insert(key, record);
        }
    }

    /// Record a 304 Not Modified response (no content change)
    pub fn record_not_modified(&self, url: &str) {
        let key = normalize_url(url);
        let mut records = self.records.write();

        if let Some(existing) = records.get_mut(&key) {
            existing.last_crawled_at = Utc::now();
            existing.crawl_count += 1;
            // Content didn't change, increase interval
            let new_interval =
                (existing.recrawl_interval_secs as f64 * self.config.unchanged_multiplier) as u64;
            existing.recrawl_interval_secs =
                new_interval.min(self.config.max_recrawl_interval.as_secs());
        }
    }

    /// Check if a URL should be re-crawled based on its history
    pub fn should_recrawl(&self, url: &str) -> bool {
        let key = normalize_url(url);
        let records = self.records.read();

        match records.get(&key) {
            Some(record) => {
                let elapsed = Utc::now()
                    .signed_duration_since(record.last_crawled_at)
                    .num_seconds() as u64;
                elapsed >= record.recrawl_interval_secs
            }
            None => true, // Never crawled, should crawl
        }
    }

    /// Check if a URL should be re-crawled with a custom minimum interval
    pub fn should_recrawl_with_interval(&self, url: &str, min_interval: Duration) -> bool {
        let key = normalize_url(url);
        let records = self.records.read();

        match records.get(&key) {
            Some(record) => {
                let elapsed = Utc::now()
                    .signed_duration_since(record.last_crawled_at)
                    .num_seconds() as u64;
                let effective_interval = record.recrawl_interval_secs.max(min_interval.as_secs());
                elapsed >= effective_interval
            }
            None => true,
        }
    }

    /// Get time until a URL should be re-crawled
    pub fn time_until_recrawl(&self, url: &str) -> Option<Duration> {
        let key = normalize_url(url);
        let records = self.records.read();

        records.get(&key).map(|record| {
            let elapsed = Utc::now()
                .signed_duration_since(record.last_crawled_at)
                .num_seconds() as u64;
            if elapsed >= record.recrawl_interval_secs {
                Duration::from_secs(0)
            } else {
                Duration::from_secs(record.recrawl_interval_secs - elapsed)
            }
        })
    }

    /// Get the conditional request headers for a URL
    pub fn get_conditional_headers(&self, url: &str) -> ConditionalHeaders {
        let key = normalize_url(url);
        let records = self.records.read();

        match records.get(&key) {
            Some(record) => ConditionalHeaders {
                if_none_match: record.etag.clone(),
                if_modified_since: record.last_modified.clone(),
            },
            None => ConditionalHeaders::default(),
        }
    }

    /// Get statistics about the history tracker
    pub fn stats(&self) -> UrlHistoryStats {
        let records = self.records.read();
        let total_count = records.len();
        let total_crawls: u64 = records.values().map(|r| r.crawl_count as u64).sum();
        let total_changes: u64 = records.values().map(|r| r.change_count as u64).sum();

        let avg_change_rate = if total_crawls > 0 {
            total_changes as f64 / total_crawls as f64
        } else {
            0.0
        };

        let avg_interval_secs = if total_count > 0 {
            records
                .values()
                .map(|r| r.recrawl_interval_secs)
                .sum::<u64>()
                / total_count as u64
        } else {
            0
        };

        UrlHistoryStats {
            tracked_urls: total_count,
            total_crawls,
            total_changes,
            avg_change_rate,
            avg_recrawl_interval_secs: avg_interval_secs,
        }
    }

    /// Clear all history
    pub fn clear(&self) {
        self.records.write().clear();
    }

    /// Get count of tracked URLs
    pub fn len(&self) -> usize {
        self.records.read().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.records.read().is_empty()
    }
}

/// Conditional HTTP headers for incremental crawling
#[derive(Debug, Clone, Default)]
pub struct ConditionalHeaders {
    /// Value for If-None-Match header (from ETag)
    pub if_none_match: Option<String>,
    /// Value for If-Modified-Since header
    pub if_modified_since: Option<String>,
}

impl ConditionalHeaders {
    /// Check if any conditional headers are available
    pub fn has_headers(&self) -> bool {
        self.if_none_match.is_some() || self.if_modified_since.is_some()
    }
}

/// Statistics about URL history
#[derive(Debug, Clone)]
pub struct UrlHistoryStats {
    /// Number of URLs being tracked
    pub tracked_urls: usize,
    /// Total number of crawls across all URLs
    pub total_crawls: u64,
    /// Total number of content changes detected
    pub total_changes: u64,
    /// Average change rate across all URLs
    pub avg_change_rate: f64,
    /// Average re-crawl interval in seconds
    pub avg_recrawl_interval_secs: u64,
}

/// Generate a content fingerprint (SHA-256 hash)
pub fn fingerprint_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Generate a content fingerprint from bytes
pub fn fingerprint_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

/// Normalize a URL for consistent key lookup
fn normalize_url(url: &str) -> String {
    // Simple normalization: lowercase the scheme and host
    // In production, use a proper URL normalization library
    url.trim().to_lowercase()
}

/// Result of checking if content has changed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentChangeResult {
    /// Content has changed since last crawl
    Changed,
    /// Content is the same as last crawl
    Unchanged,
    /// No previous crawl to compare against
    FirstCrawl,
    /// HTTP 304 Not Modified response
    NotModified,
}

/// Check if content has changed based on hash comparison
pub fn check_content_change(
    history: &UrlHistory,
    url: &str,
    new_content: &str,
) -> ContentChangeResult {
    let record = history.get_record(url);
    let new_hash = fingerprint_content(new_content);

    match record {
        Some(existing) => match existing.content_hash {
            Some(ref old_hash) if old_hash == &new_hash => ContentChangeResult::Unchanged,
            Some(_) => ContentChangeResult::Changed,
            None => ContentChangeResult::Changed, // No previous hash, treat as changed
        },
        None => ContentChangeResult::FirstCrawl,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_crawl_record_creation() {
        let record = CrawlRecord::new()
            .with_etag("abc123")
            .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT")
            .with_content_hash("sha256:abcdef")
            .with_status(200)
            .with_content_length(1024);

        assert_eq!(record.etag, Some("abc123".to_string()));
        assert_eq!(
            record.last_modified,
            Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
        );
        assert_eq!(record.content_hash, Some("sha256:abcdef".to_string()));
        assert_eq!(record.last_status, Some(200));
        assert_eq!(record.last_content_length, Some(1024));
        assert_eq!(record.crawl_count, 1);
        assert_eq!(record.change_count, 0);
    }

    #[test]
    fn test_url_history_basic() {
        let history = UrlHistory::with_defaults();

        // First crawl
        let record = CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com/page", record);

        // Should have record
        let stored = history.get_record("https://example.com/page");
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().crawl_count, 1);
    }

    #[test]
    fn test_url_history_update() {
        let history = UrlHistory::with_defaults();

        // First crawl
        let record1 = CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com/page", record1);

        // Second crawl with same content
        let record2 = CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com/page", record2);

        let stored = history.get_record("https://example.com/page").unwrap();
        assert_eq!(stored.crawl_count, 2);
        assert_eq!(stored.change_count, 0); // Content unchanged

        // Third crawl with different content
        let record3 = CrawlRecord::new().with_content_hash("hash2");
        history.record_crawl("https://example.com/page", record3);

        let stored = history.get_record("https://example.com/page").unwrap();
        assert_eq!(stored.crawl_count, 3);
        assert_eq!(stored.change_count, 1); // Content changed once
    }

    #[test]
    fn test_should_recrawl() {
        let config = UrlHistoryConfig {
            default_recrawl_interval: Duration::from_secs(1), // Use seconds
            ..Default::default()
        };
        let history = UrlHistory::new(config);

        // Never crawled - should recrawl
        assert!(history.should_recrawl("https://example.com/new"));

        // Just crawled - should not recrawl (recrawl_interval_secs = 1)
        let record = CrawlRecord::new();
        history.record_crawl("https://example.com/page", record);
        assert!(!history.should_recrawl("https://example.com/page"));

        // Wait for interval to pass
        sleep(Duration::from_millis(1100)); // 1.1 seconds > 1 second interval
        assert!(history.should_recrawl("https://example.com/page"));
    }

    #[test]
    fn test_conditional_headers() {
        let history = UrlHistory::with_defaults();

        // No headers for unknown URL
        let headers = history.get_conditional_headers("https://example.com/new");
        assert!(!headers.has_headers());

        // Record with headers
        let record = CrawlRecord::new()
            .with_etag("\"abc123\"")
            .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT");
        history.record_crawl("https://example.com/page", record);

        let headers = history.get_conditional_headers("https://example.com/page");
        assert!(headers.has_headers());
        assert_eq!(headers.if_none_match, Some("\"abc123\"".to_string()));
        assert_eq!(
            headers.if_modified_since,
            Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
        );
    }

    #[test]
    fn test_fingerprint_content() {
        let hash1 = fingerprint_content("Hello, World!");
        let hash2 = fingerprint_content("Hello, World!");
        let hash3 = fingerprint_content("Different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert!(hash1.starts_with("sha256:"));
    }

    #[test]
    fn test_content_change_detection() {
        let history = UrlHistory::with_defaults();

        // First crawl
        assert_eq!(
            check_content_change(&history, "https://example.com", "content"),
            ContentChangeResult::FirstCrawl
        );

        // Record the crawl
        let hash = fingerprint_content("content");
        let record = CrawlRecord::new().with_content_hash(hash);
        history.record_crawl("https://example.com", record);

        // Same content
        assert_eq!(
            check_content_change(&history, "https://example.com", "content"),
            ContentChangeResult::Unchanged
        );

        // Different content
        assert_eq!(
            check_content_change(&history, "https://example.com", "new content"),
            ContentChangeResult::Changed
        );
    }

    #[test]
    fn test_adaptive_intervals() {
        let config = UrlHistoryConfig {
            default_recrawl_interval: Duration::from_secs(100),
            min_recrawl_interval: Duration::from_secs(10),
            max_recrawl_interval: Duration::from_secs(1000),
            unchanged_multiplier: 2.0,
            changed_multiplier: 0.5,
            ..Default::default()
        };
        let history = UrlHistory::new(config);

        // First crawl
        let record1 = CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com", record1);

        // Check initial interval
        let stored = history.get_record("https://example.com").unwrap();
        assert_eq!(stored.recrawl_interval_secs, 100);

        // Second crawl - unchanged (interval should increase)
        let record2 = CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com", record2);

        let stored = history.get_record("https://example.com").unwrap();
        assert_eq!(stored.recrawl_interval_secs, 200); // 100 * 2.0

        // Third crawl - changed (interval should decrease)
        let record3 = CrawlRecord::new().with_content_hash("hash2");
        history.record_crawl("https://example.com", record3);

        let stored = history.get_record("https://example.com").unwrap();
        assert_eq!(stored.recrawl_interval_secs, 100); // 200 * 0.5
    }

    #[test]
    fn test_stats() {
        let history = UrlHistory::with_defaults();

        // Add some records
        for i in 0..5 {
            let record = CrawlRecord::new().with_content_hash(format!("hash{}", i));
            history.record_crawl(&format!("https://example{}.com", i), record);
        }

        let stats = history.stats();
        assert_eq!(stats.tracked_urls, 5);
        assert_eq!(stats.total_crawls, 5);
        assert_eq!(stats.total_changes, 0);
    }

    #[test]
    fn test_not_modified_recording() {
        let config = UrlHistoryConfig {
            default_recrawl_interval: Duration::from_secs(100),
            unchanged_multiplier: 2.0,
            max_recrawl_interval: Duration::from_secs(10000),
            ..Default::default()
        };
        let history = UrlHistory::new(config);

        // Initial crawl
        let record = CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com", record);

        // Record 304 Not Modified
        history.record_not_modified("https://example.com");

        let stored = history.get_record("https://example.com").unwrap();
        assert_eq!(stored.crawl_count, 2);
        assert_eq!(stored.change_count, 0);
        assert_eq!(stored.recrawl_interval_secs, 200); // Increased due to no change
    }
}
