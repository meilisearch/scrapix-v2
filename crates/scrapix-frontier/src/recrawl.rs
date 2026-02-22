//! Re-crawl scheduler for incremental crawling
//!
//! This module provides scheduling logic for determining when URLs should be re-crawled
//! based on their crawl history and content change patterns.
//!
//! ## Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use scrapix_frontier::{
//!     RecrawlScheduler, RecrawlConfig, RecrawlDecision, UrlHistory, UrlHistoryConfig,
//! };
//! use scrapix_core::CrawlUrl;
//!
//! // Create history tracker
//! let history = Arc::new(UrlHistory::new(UrlHistoryConfig::default()));
//!
//! // Create re-crawl scheduler
//! let scheduler = RecrawlScheduler::new(RecrawlConfig::default(), history);
//!
//! // Check if URL needs re-crawl
//! let url = CrawlUrl::seed("https://example.com/page");
//! let decision = scheduler.should_crawl(&url);
//!
//! match decision {
//!     RecrawlDecision::Crawl { reason, .. } => {
//!         println!("Should crawl: {}", reason);
//!     }
//!     RecrawlDecision::Skip { reason, .. } => {
//!         println!("Skip for now: {}", reason);
//!     }
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

use scrapix_core::CrawlUrl;

use crate::history::{CrawlRecord, UrlHistory};

/// Configuration for the re-crawl scheduler
#[derive(Debug, Clone)]
pub struct RecrawlConfig {
    /// Whether to enable incremental crawling
    pub enabled: bool,
    /// Force re-crawl interval (override adaptive scheduling)
    pub force_recrawl_interval: Option<Duration>,
    /// Maximum age before forcing re-crawl regardless of change rate
    pub max_age: Duration,
    /// Minimum age before allowing re-crawl
    pub min_age: Duration,
    /// Priority boost for high-change-rate URLs (0.0 = no boost)
    pub change_rate_priority_boost: f64,
    /// Skip re-crawl if URL has high error rate
    pub skip_high_error_rate: bool,
    /// Error rate threshold for skipping (0.0-1.0)
    pub error_rate_threshold: f64,
}

impl Default for RecrawlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            force_recrawl_interval: None,
            max_age: Duration::from_secs(7 * 24 * 3600), // 7 days
            min_age: Duration::from_secs(3600),          // 1 hour
            change_rate_priority_boost: 10.0,
            skip_high_error_rate: true,
            error_rate_threshold: 0.5,
        }
    }
}

/// Decision about whether to re-crawl a URL
#[derive(Debug, Clone)]
pub enum RecrawlDecision {
    /// Should crawl the URL
    Crawl {
        /// Reason for crawling
        reason: RecrawlReason,
        /// Suggested priority boost (higher = more important)
        priority_boost: i32,
        /// Whether to use conditional request headers
        use_conditional: bool,
    },
    /// Should skip this URL for now
    Skip {
        /// Reason for skipping
        reason: SkipReason,
        /// When to retry (if known)
        retry_after: Option<Duration>,
    },
}

/// Reason for deciding to crawl
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecrawlReason {
    /// Never crawled before
    FirstCrawl,
    /// Adaptive interval has elapsed
    IntervalElapsed,
    /// Maximum age exceeded
    MaxAgeExceeded,
    /// High change rate - crawl more frequently
    HighChangeRate,
    /// Force re-crawl interval configured
    ForcedRecrawl,
}

impl std::fmt::Display for RecrawlReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FirstCrawl => write!(f, "first_crawl"),
            Self::IntervalElapsed => write!(f, "interval_elapsed"),
            Self::MaxAgeExceeded => write!(f, "max_age_exceeded"),
            Self::HighChangeRate => write!(f, "high_change_rate"),
            Self::ForcedRecrawl => write!(f, "forced_recrawl"),
        }
    }
}

/// Reason for deciding to skip
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// Recently crawled, within minimum age
    TooRecent,
    /// Incremental crawling disabled
    Disabled,
    /// High error rate on this URL
    HighErrorRate,
    /// Custom skip reason
    Custom(String),
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooRecent => write!(f, "too_recent"),
            Self::Disabled => write!(f, "disabled"),
            Self::HighErrorRate => write!(f, "high_error_rate"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Re-crawl scheduler using URL history
pub struct RecrawlScheduler {
    config: RecrawlConfig,
    history: Arc<UrlHistory>,
}

impl RecrawlScheduler {
    /// Create a new re-crawl scheduler
    pub fn new(config: RecrawlConfig, history: Arc<UrlHistory>) -> Self {
        Self { config, history }
    }

    /// Create with default configuration
    pub fn with_defaults(history: Arc<UrlHistory>) -> Self {
        Self::new(RecrawlConfig::default(), history)
    }

    /// Determine if a URL should be crawled
    pub fn should_crawl(&self, url: &CrawlUrl) -> RecrawlDecision {
        if !self.config.enabled {
            return RecrawlDecision::Skip {
                reason: SkipReason::Disabled,
                retry_after: None,
            };
        }

        let record = self.history.get_record(&url.url);

        match record {
            None => {
                // Never crawled - definitely crawl
                RecrawlDecision::Crawl {
                    reason: RecrawlReason::FirstCrawl,
                    priority_boost: 0,
                    use_conditional: false,
                }
            }
            Some(record) => self.evaluate_existing_record(&url.url, &record),
        }
    }

    /// Evaluate whether to re-crawl based on existing record
    fn evaluate_existing_record(&self, _url: &str, record: &CrawlRecord) -> RecrawlDecision {
        let now = chrono::Utc::now();
        let age_secs = now
            .signed_duration_since(record.last_crawled_at)
            .num_seconds() as u64;

        // Check minimum age
        if age_secs < self.config.min_age.as_secs() {
            return RecrawlDecision::Skip {
                reason: SkipReason::TooRecent,
                retry_after: Some(Duration::from_secs(
                    self.config.min_age.as_secs() - age_secs,
                )),
            };
        }

        // Check for force recrawl interval
        if let Some(force_interval) = self.config.force_recrawl_interval {
            if age_secs >= force_interval.as_secs() {
                return RecrawlDecision::Crawl {
                    reason: RecrawlReason::ForcedRecrawl,
                    priority_boost: 0,
                    use_conditional: record.has_caching_headers(),
                };
            }
        }

        // Check max age
        if age_secs >= self.config.max_age.as_secs() {
            return RecrawlDecision::Crawl {
                reason: RecrawlReason::MaxAgeExceeded,
                priority_boost: 5, // Higher priority for stale content
                use_conditional: record.has_caching_headers(),
            };
        }

        // Check adaptive interval
        if age_secs >= record.recrawl_interval_secs {
            let change_rate = record.change_rate();
            let priority_boost = if change_rate > 0.5 {
                // High change rate - boost priority
                (self.config.change_rate_priority_boost * change_rate) as i32
            } else {
                0
            };

            return RecrawlDecision::Crawl {
                reason: if change_rate > 0.5 {
                    RecrawlReason::HighChangeRate
                } else {
                    RecrawlReason::IntervalElapsed
                },
                priority_boost,
                use_conditional: record.has_caching_headers(),
            };
        }

        // Not yet time to re-crawl
        let retry_after = Duration::from_secs(record.recrawl_interval_secs - age_secs);
        RecrawlDecision::Skip {
            reason: SkipReason::TooRecent,
            retry_after: Some(retry_after),
        }
    }

    /// Get URLs that are due for re-crawl from a batch
    pub fn filter_for_recrawl(&self, urls: Vec<CrawlUrl>) -> Vec<(CrawlUrl, RecrawlDecision)> {
        urls.into_iter()
            .map(|url| {
                let decision = self.should_crawl(&url);
                (url, decision)
            })
            .filter(|(_, decision)| matches!(decision, RecrawlDecision::Crawl { .. }))
            .collect()
    }

    /// Apply priority boost based on re-crawl decision
    pub fn apply_priority_boost(&self, mut url: CrawlUrl) -> CrawlUrl {
        if let RecrawlDecision::Crawl { priority_boost, .. } = self.should_crawl(&url) {
            url.priority += priority_boost;
        }
        url
    }

    /// Get estimated time until next re-crawl for a URL
    pub fn time_until_recrawl(&self, url: &str) -> Option<Duration> {
        self.history.time_until_recrawl(url)
    }

    /// Get statistics about re-crawl scheduling
    pub fn stats(&self) -> RecrawlStats {
        let history_stats = self.history.stats();

        RecrawlStats {
            tracked_urls: history_stats.tracked_urls,
            total_crawls: history_stats.total_crawls,
            total_changes: history_stats.total_changes,
            avg_change_rate: history_stats.avg_change_rate,
            avg_recrawl_interval_secs: history_stats.avg_recrawl_interval_secs,
        }
    }

    /// Get the underlying history tracker
    pub fn history(&self) -> &Arc<UrlHistory> {
        &self.history
    }
}

/// Statistics about re-crawl scheduling
#[derive(Debug, Clone)]
pub struct RecrawlStats {
    /// Number of URLs being tracked
    pub tracked_urls: usize,
    /// Total number of crawls
    pub total_crawls: u64,
    /// Total number of content changes detected
    pub total_changes: u64,
    /// Average change rate across URLs
    pub avg_change_rate: f64,
    /// Average re-crawl interval in seconds
    pub avg_recrawl_interval_secs: u64,
}

/// Builder for RecrawlScheduler
pub struct RecrawlSchedulerBuilder {
    config: RecrawlConfig,
}

impl RecrawlSchedulerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: RecrawlConfig::default(),
        }
    }

    /// Enable or disable incremental crawling
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    /// Set force re-crawl interval
    pub fn force_recrawl_interval(mut self, interval: Duration) -> Self {
        self.config.force_recrawl_interval = Some(interval);
        self
    }

    /// Set maximum age before forcing re-crawl
    pub fn max_age(mut self, max_age: Duration) -> Self {
        self.config.max_age = max_age;
        self
    }

    /// Set minimum age before allowing re-crawl
    pub fn min_age(mut self, min_age: Duration) -> Self {
        self.config.min_age = min_age;
        self
    }

    /// Set priority boost for high-change-rate URLs
    pub fn change_rate_priority_boost(mut self, boost: f64) -> Self {
        self.config.change_rate_priority_boost = boost;
        self
    }

    /// Build the scheduler
    pub fn build(self, history: Arc<UrlHistory>) -> RecrawlScheduler {
        RecrawlScheduler::new(self.config, history)
    }
}

impl Default for RecrawlSchedulerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::UrlHistoryConfig;
    use std::thread::sleep;

    fn create_test_scheduler() -> (RecrawlScheduler, Arc<UrlHistory>) {
        // Use seconds for intervals (recrawl_interval_secs is stored in seconds)
        let history_config = UrlHistoryConfig {
            default_recrawl_interval: Duration::from_secs(1), // 1 second
            min_recrawl_interval: Duration::from_secs(1),     // 1 second
            max_recrawl_interval: Duration::from_secs(10),
            ..Default::default()
        };
        let history = Arc::new(UrlHistory::new(history_config));

        let config = RecrawlConfig {
            min_age: Duration::from_secs(1), // 1 second min age
            max_age: Duration::from_secs(10),
            ..Default::default()
        };
        let scheduler = RecrawlScheduler::new(config, history.clone());

        (scheduler, history)
    }

    #[test]
    fn test_first_crawl() {
        let (scheduler, _) = create_test_scheduler();

        let url = CrawlUrl::seed("https://example.com/new");
        let decision = scheduler.should_crawl(&url);

        match decision {
            RecrawlDecision::Crawl { reason, .. } => {
                assert_eq!(reason, RecrawlReason::FirstCrawl);
            }
            _ => panic!("Expected Crawl decision for new URL"),
        }
    }

    #[test]
    fn test_too_recent() {
        let (scheduler, history) = create_test_scheduler();

        // Record a crawl
        let record = crate::history::CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com/page", record);

        // Immediately check - should skip (too recent)
        let url = CrawlUrl::seed("https://example.com/page");
        let decision = scheduler.should_crawl(&url);

        match decision {
            RecrawlDecision::Skip { reason, .. } => {
                assert_eq!(reason, SkipReason::TooRecent);
            }
            _ => panic!("Expected Skip decision for recent URL"),
        }
    }

    #[test]
    fn test_interval_elapsed() {
        let (scheduler, history) = create_test_scheduler();

        // Record a crawl
        let record = crate::history::CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com/page", record);

        // Wait for interval to elapse (1 second + buffer)
        sleep(Duration::from_millis(1200));

        // Now should crawl
        let url = CrawlUrl::seed("https://example.com/page");
        let decision = scheduler.should_crawl(&url);

        match decision {
            RecrawlDecision::Crawl { reason, .. } => {
                assert_eq!(reason, RecrawlReason::IntervalElapsed);
            }
            _ => panic!(
                "Expected Crawl decision after interval elapsed, got {:?}",
                decision
            ),
        }
    }

    #[test]
    fn test_conditional_headers() {
        let (scheduler, history) = create_test_scheduler();

        // Record a crawl with caching headers
        let record = crate::history::CrawlRecord::new()
            .with_etag("\"abc123\"")
            .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT")
            .with_content_hash("hash1");
        history.record_crawl("https://example.com/page", record);

        // Wait for interval (1 second + buffer)
        sleep(Duration::from_millis(1200));

        // Check decision
        let url = CrawlUrl::seed("https://example.com/page");
        let decision = scheduler.should_crawl(&url);

        match decision {
            RecrawlDecision::Crawl {
                use_conditional, ..
            } => {
                assert!(use_conditional, "Should use conditional headers");
            }
            _ => panic!("Expected Crawl decision, got {:?}", decision),
        }
    }

    #[test]
    fn test_disabled_scheduler() {
        let history = Arc::new(UrlHistory::with_defaults());
        let config = RecrawlConfig {
            enabled: false,
            ..Default::default()
        };
        let scheduler = RecrawlScheduler::new(config, history);

        let url = CrawlUrl::seed("https://example.com/page");
        let decision = scheduler.should_crawl(&url);

        match decision {
            RecrawlDecision::Skip { reason, .. } => {
                assert_eq!(reason, SkipReason::Disabled);
            }
            _ => panic!("Expected Skip decision when disabled"),
        }
    }

    #[test]
    fn test_filter_for_recrawl() {
        let (scheduler, history) = create_test_scheduler();

        // Add one existing URL that was recently crawled
        let record = crate::history::CrawlRecord::new().with_content_hash("hash1");
        history.record_crawl("https://example.com/recent", record);

        // Create batch of URLs
        let urls = vec![
            CrawlUrl::seed("https://example.com/new1"),
            CrawlUrl::seed("https://example.com/new2"),
            CrawlUrl::seed("https://example.com/recent"),
        ];

        let filtered = scheduler.filter_for_recrawl(urls);

        // Should only include new URLs (not the recent one)
        assert_eq!(filtered.len(), 2);
        assert!(filtered
            .iter()
            .all(|(url, _)| url.url != "https://example.com/recent"));
    }
}
