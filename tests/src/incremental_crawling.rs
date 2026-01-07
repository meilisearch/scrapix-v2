//! Integration tests for incremental crawling
//!
//! These tests verify:
//! - URL history tracking works correctly
//! - Conditional HTTP headers are generated properly
//! - Content fingerprinting detects changes
//! - Re-crawl scheduling adapts to content change rates
//! - Integration between history, scheduler, and fetcher works

use scrapix_core::CrawlUrl;
use scrapix_crawler::ConditionalRequestHeaders;
use scrapix_frontier::{
    check_content_change, fingerprint_content, ContentChangeResult, CrawlRecord, RecrawlConfig,
    RecrawlDecision, RecrawlReason, RecrawlScheduler, SkipReason, UrlHistory, UrlHistoryConfig,
};
use scrapix_tests::init_test_tracing;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// URL History Tests
// ============================================================================

#[test]
fn test_url_history_first_crawl() {
    init_test_tracing();

    let history = UrlHistory::with_defaults();

    // New URL should not have a record
    assert!(history.get_record("https://example.com/new").is_none());

    // Record first crawl
    let record = CrawlRecord::new()
        .with_etag("\"abc123\"")
        .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT")
        .with_content_hash(fingerprint_content("Hello World"))
        .with_status(200)
        .with_content_length(11);

    history.record_crawl("https://example.com/new", record);

    // Now should have record
    let stored = history.get_record("https://example.com/new").unwrap();
    assert_eq!(stored.crawl_count, 1);
    assert_eq!(stored.change_count, 0);
    assert_eq!(stored.etag, Some("\"abc123\"".to_string()));
    assert!(stored.content_hash.is_some());
}

#[test]
fn test_url_history_content_unchanged() {
    init_test_tracing();

    let history = UrlHistory::with_defaults();

    // First crawl
    let hash = fingerprint_content("Same content");
    let record1 = CrawlRecord::new().with_content_hash(&hash);
    history.record_crawl("https://example.com/page", record1);

    // Second crawl with same content
    let record2 = CrawlRecord::new().with_content_hash(&hash);
    history.record_crawl("https://example.com/page", record2);

    let stored = history.get_record("https://example.com/page").unwrap();
    assert_eq!(stored.crawl_count, 2);
    assert_eq!(stored.change_count, 0); // No changes
}

#[test]
fn test_url_history_content_changed() {
    init_test_tracing();

    let history = UrlHistory::with_defaults();

    // First crawl
    let hash1 = fingerprint_content("Content version 1");
    let record1 = CrawlRecord::new().with_content_hash(&hash1);
    history.record_crawl("https://example.com/page", record1);

    // Second crawl with different content
    let hash2 = fingerprint_content("Content version 2");
    let record2 = CrawlRecord::new().with_content_hash(&hash2);
    history.record_crawl("https://example.com/page", record2);

    let stored = history.get_record("https://example.com/page").unwrap();
    assert_eq!(stored.crawl_count, 2);
    assert_eq!(stored.change_count, 1); // One change
}

#[test]
fn test_url_history_adaptive_interval() {
    init_test_tracing();

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
    history.record_crawl("https://example.com/page", record1);

    // Initial interval should be default
    let stored = history.get_record("https://example.com/page").unwrap();
    assert_eq!(stored.recrawl_interval_secs, 100);

    // Second crawl - same content (interval increases)
    let record2 = CrawlRecord::new().with_content_hash("hash1");
    history.record_crawl("https://example.com/page", record2);

    let stored = history.get_record("https://example.com/page").unwrap();
    assert_eq!(stored.recrawl_interval_secs, 200); // 100 * 2.0

    // Third crawl - different content (interval decreases)
    let record3 = CrawlRecord::new().with_content_hash("hash2");
    history.record_crawl("https://example.com/page", record3);

    let stored = history.get_record("https://example.com/page").unwrap();
    assert_eq!(stored.recrawl_interval_secs, 100); // 200 * 0.5
}

#[test]
fn test_url_history_conditional_headers() {
    init_test_tracing();

    let history = UrlHistory::with_defaults();

    // No headers for unknown URL
    let headers = history.get_conditional_headers("https://example.com/unknown");
    assert!(!headers.has_headers());

    // Record with headers
    let record = CrawlRecord::new()
        .with_etag("\"etag-value\"")
        .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT");
    history.record_crawl("https://example.com/page", record);

    // Should get headers back
    let headers = history.get_conditional_headers("https://example.com/page");
    assert!(headers.has_headers());
    assert_eq!(headers.if_none_match, Some("\"etag-value\"".to_string()));
    assert_eq!(
        headers.if_modified_since,
        Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
    );
}

// ============================================================================
// Content Fingerprinting Tests
// ============================================================================

#[test]
fn test_fingerprint_consistency() {
    let content = "This is test content for fingerprinting";

    let hash1 = fingerprint_content(content);
    let hash2 = fingerprint_content(content);

    assert_eq!(hash1, hash2);
    assert!(hash1.starts_with("sha256:"));
}

#[test]
fn test_fingerprint_uniqueness() {
    let hash1 = fingerprint_content("Content A");
    let hash2 = fingerprint_content("Content B");

    assert_ne!(hash1, hash2);
}

#[test]
fn test_content_change_detection() {
    init_test_tracing();

    let history = UrlHistory::with_defaults();

    // First crawl - should be FirstCrawl
    let result1 = check_content_change(&history, "https://example.com/page", "Initial content");
    assert_eq!(result1, ContentChangeResult::FirstCrawl);

    // Record the crawl
    let hash = fingerprint_content("Initial content");
    let record = CrawlRecord::new().with_content_hash(hash);
    history.record_crawl("https://example.com/page", record);

    // Same content - should be Unchanged
    let result2 = check_content_change(&history, "https://example.com/page", "Initial content");
    assert_eq!(result2, ContentChangeResult::Unchanged);

    // Different content - should be Changed
    let result3 = check_content_change(&history, "https://example.com/page", "Updated content");
    assert_eq!(result3, ContentChangeResult::Changed);
}

// ============================================================================
// Re-crawl Scheduler Tests
// ============================================================================

#[test]
fn test_scheduler_first_crawl() {
    init_test_tracing();

    let history = Arc::new(UrlHistory::with_defaults());
    let scheduler = RecrawlScheduler::with_defaults(history);

    let url = CrawlUrl::seed("https://example.com/new");
    let decision = scheduler.should_crawl(&url);

    match decision {
        RecrawlDecision::Crawl {
            reason,
            use_conditional,
            ..
        } => {
            assert_eq!(reason, RecrawlReason::FirstCrawl);
            assert!(!use_conditional); // No previous crawl, no conditional headers
        }
        _ => panic!("Expected Crawl decision for new URL"),
    }
}

#[test]
fn test_scheduler_too_recent() {
    init_test_tracing();

    let history_config = UrlHistoryConfig {
        default_recrawl_interval: Duration::from_secs(3600),
        ..Default::default()
    };
    let history = Arc::new(UrlHistory::new(history_config));

    let scheduler_config = RecrawlConfig {
        min_age: Duration::from_secs(60),
        ..Default::default()
    };
    let scheduler = RecrawlScheduler::new(scheduler_config, history.clone());

    // Record a recent crawl
    let record = CrawlRecord::new().with_content_hash("hash");
    history.record_crawl("https://example.com/page", record);

    // Should skip - too recent
    let url = CrawlUrl::seed("https://example.com/page");
    let decision = scheduler.should_crawl(&url);

    match decision {
        RecrawlDecision::Skip {
            reason,
            retry_after,
        } => {
            assert_eq!(reason, SkipReason::TooRecent);
            assert!(retry_after.is_some());
        }
        _ => panic!("Expected Skip decision for recent URL"),
    }
}

#[test]
fn test_scheduler_disabled() {
    init_test_tracing();

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
fn test_scheduler_with_conditional_headers() {
    init_test_tracing();

    let history_config = UrlHistoryConfig {
        default_recrawl_interval: Duration::from_millis(50),
        min_recrawl_interval: Duration::from_millis(10),
        ..Default::default()
    };
    let history = Arc::new(UrlHistory::new(history_config));

    let scheduler_config = RecrawlConfig {
        min_age: Duration::from_millis(10),
        ..Default::default()
    };
    let scheduler = RecrawlScheduler::new(scheduler_config, history.clone());

    // Record a crawl with caching headers
    let record = CrawlRecord::new()
        .with_etag("\"abc\"")
        .with_last_modified("date-string")
        .with_content_hash("hash");
    history.record_crawl("https://example.com/page", record);

    // Wait for interval
    std::thread::sleep(Duration::from_millis(100));

    // Should crawl with conditional headers
    let url = CrawlUrl::seed("https://example.com/page");
    let decision = scheduler.should_crawl(&url);

    match decision {
        RecrawlDecision::Crawl {
            use_conditional, ..
        } => {
            assert!(use_conditional, "Should use conditional headers");
        }
        _ => panic!("Expected Crawl decision"),
    }
}

#[test]
fn test_scheduler_filter_for_recrawl() {
    init_test_tracing();

    let history_config = UrlHistoryConfig {
        default_recrawl_interval: Duration::from_secs(3600),
        ..Default::default()
    };
    let history = Arc::new(UrlHistory::new(history_config));

    let scheduler_config = RecrawlConfig {
        min_age: Duration::from_secs(60),
        ..Default::default()
    };
    let scheduler = RecrawlScheduler::new(scheduler_config, history.clone());

    // Record a recent crawl for one URL
    let record = CrawlRecord::new().with_content_hash("hash");
    history.record_crawl("https://example.com/recent", record);

    // Create batch of URLs
    let urls = vec![
        CrawlUrl::seed("https://example.com/new1"),
        CrawlUrl::seed("https://example.com/new2"),
        CrawlUrl::seed("https://example.com/recent"),
    ];

    let filtered = scheduler.filter_for_recrawl(urls);

    // Should filter out the recent URL
    assert_eq!(filtered.len(), 2);
    assert!(filtered
        .iter()
        .all(|(url, _)| url.url != "https://example.com/recent"));
}

// ============================================================================
// Conditional Request Headers Tests
// ============================================================================

#[test]
fn test_conditional_request_headers() {
    let headers = ConditionalRequestHeaders::new()
        .with_etag("\"abc123\"")
        .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT");

    assert!(headers.has_headers());
    assert_eq!(headers.etag, Some("\"abc123\"".to_string()));
    assert_eq!(
        headers.last_modified,
        Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
    );
}

#[test]
fn test_empty_conditional_headers() {
    let headers = ConditionalRequestHeaders::new();
    assert!(!headers.has_headers());
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_full_incremental_crawl_workflow() {
    init_test_tracing();

    // Setup - use seconds for intervals (recrawl_interval_secs is in seconds)
    let history_config = UrlHistoryConfig {
        default_recrawl_interval: Duration::from_secs(2), // 2 seconds
        min_recrawl_interval: Duration::from_secs(1),     // 1 second
        unchanged_multiplier: 2.0,
        changed_multiplier: 0.5,
        ..Default::default()
    };
    let history = Arc::new(UrlHistory::new(history_config));

    let scheduler_config = RecrawlConfig {
        min_age: Duration::from_secs(1), // 1 second min age
        ..Default::default()
    };
    let scheduler = RecrawlScheduler::new(scheduler_config, history.clone());

    let url = CrawlUrl::seed("https://example.com/page");

    // Step 1: First crawl - should be allowed
    let decision1 = scheduler.should_crawl(&url);
    assert!(
        matches!(
            decision1,
            RecrawlDecision::Crawl {
                reason: RecrawlReason::FirstCrawl,
                ..
            }
        ),
        "Expected FirstCrawl, got {:?}",
        decision1
    );

    // Simulate fetching and record the crawl
    let content1 = "Initial page content";
    let hash1 = fingerprint_content(content1);
    let record1 = CrawlRecord::new()
        .with_etag("\"v1\"")
        .with_content_hash(&hash1)
        .with_status(200);
    history.record_crawl(&url.url, record1);

    // Step 2: Immediate re-check - should be too recent (< 1 second min_age)
    let decision2 = scheduler.should_crawl(&url);
    assert!(
        matches!(
            decision2,
            RecrawlDecision::Skip {
                reason: SkipReason::TooRecent,
                ..
            }
        ),
        "Expected TooRecent, got {:?}",
        decision2
    );

    // Step 3: Wait for min_age (1s) to pass but not recrawl interval (2s)
    std::thread::sleep(Duration::from_millis(1200));
    let decision3 = scheduler.should_crawl(&url);
    // min_age passed, but recrawl_interval (2s) hasn't
    assert!(
        matches!(
            decision3,
            RecrawlDecision::Skip {
                reason: SkipReason::TooRecent,
                ..
            }
        ),
        "Expected TooRecent (within interval), got {:?}",
        decision3
    );

    // Step 4: Wait for full recrawl interval to pass
    std::thread::sleep(Duration::from_millis(1200)); // Total ~2.4s > 2s interval
    let decision4 = scheduler.should_crawl(&url);
    assert!(
        matches!(
            decision4,
            RecrawlDecision::Crawl {
                use_conditional: true,
                ..
            }
        ),
        "Expected Crawl with conditional, got {:?}",
        decision4
    );

    // Simulate unchanged content (304 response)
    history.record_not_modified(&url.url);

    // Check record was updated
    let record = history.get_record(&url.url).unwrap();
    assert_eq!(record.crawl_count, 2);
    assert_eq!(record.change_count, 0); // No content change

    // Step 5: Wait and simulate changed content
    std::thread::sleep(Duration::from_secs(5)); // Wait for interval
    let content2 = "Updated page content";
    let hash2 = fingerprint_content(content2);
    let record2 = CrawlRecord::new()
        .with_etag("\"v2\"")
        .with_content_hash(&hash2)
        .with_status(200);
    history.record_crawl(&url.url, record2);

    // Check interval change due to content change
    let record = history.get_record(&url.url).unwrap();
    assert_eq!(record.change_count, 1);
}

#[test]
fn test_stats_tracking() {
    init_test_tracing();

    let history = Arc::new(UrlHistory::with_defaults());
    let scheduler = RecrawlScheduler::with_defaults(history.clone());

    // Add some records
    for i in 0..5 {
        let record = CrawlRecord::new().with_content_hash(format!("hash{}", i));
        history.record_crawl(&format!("https://example{}.com", i), record);
    }

    let stats = scheduler.stats();
    assert_eq!(stats.tracked_urls, 5);
    assert_eq!(stats.total_crawls, 5);
    assert_eq!(stats.total_changes, 0);
}
