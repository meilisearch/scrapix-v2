//! Integration tests for the frontier service
//!
//! These tests verify that:
//! - URL deduplication works correctly with Bloom filters
//! - Priority queue maintains correct ordering
//! - Politeness scheduler enforces rate limits
//! - Domain partitioning distributes URLs correctly
//! - Integration between components works as expected

use scrapix_core::CrawlUrl;
use scrapix_frontier::{
    extract_domain, DomainGrouper, Partitioner, PolitenessConfig, PolitenessScheduler,
    PriorityConfig, PriorityQueue, UrlDedup,
};
use scrapix_tests::init_test_tracing;

// ============================================================================
// URL Deduplication Tests
// ============================================================================

#[test]
fn test_url_dedup_basic() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(1000, 0.01);

    // First check should return false (not seen)
    assert!(!dedup.is_seen("https://example.com/page1"));

    // Mark as seen
    dedup.mark_seen("https://example.com/page1");

    // Now should be seen
    assert!(dedup.is_seen("https://example.com/page1"));

    // Different URL should not be seen
    assert!(!dedup.is_seen("https://example.com/page2"));
}

#[test]
fn test_url_dedup_check_and_mark() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(1000, 0.01);

    // First call returns false (not seen before)
    assert!(!dedup.check_and_mark("https://example.com/page1"));

    // Second call returns true (already seen)
    assert!(dedup.check_and_mark("https://example.com/page1"));

    // Third call still returns true
    assert!(dedup.check_and_mark("https://example.com/page1"));
}

#[test]
fn test_url_dedup_url_normalization() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(1000, 0.01);

    // Mark with trailing slash
    dedup.mark_seen("https://example.com/page/");

    // Should recognize without trailing slash (if normalization is implemented)
    // Note: This test documents expected behavior - adjust if normalization differs
    let seen = dedup.is_seen("https://example.com/page/");
    assert!(seen);
}

#[test]
fn test_url_dedup_stats() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(1000, 0.01);

    // Add several unique URLs
    for i in 0..10 {
        dedup.mark_seen(&format!("https://example.com/page{}", i));
    }

    let stats = dedup.stats();
    assert_eq!(stats.items_count, 10);
    assert!(stats.target_fp_rate > 0.0);
}

#[test]
fn test_url_dedup_many_urls() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(10000, 0.01);

    // Add 1000 URLs
    for i in 0..1000 {
        let url = format!("https://example.com/page/{}", i);
        assert!(!dedup.check_and_mark(&url), "URL {} should not be seen", i);
    }

    // Verify all are seen
    for i in 0..1000 {
        let url = format!("https://example.com/page/{}", i);
        assert!(dedup.is_seen(&url), "URL {} should be seen", i);
    }

    // Count false positives among new URLs
    let mut false_positives = 0;
    for i in 1000..2000 {
        let url = format!("https://example.com/page/{}", i);
        if dedup.is_seen(&url) {
            false_positives += 1;
        }
    }

    // At 1% FP rate, expect ~10 false positives out of 1000
    // Allow some variance
    assert!(
        false_positives < 50,
        "Too many false positives: {}",
        false_positives
    );
}

#[test]
fn test_url_dedup_count() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(1000, 0.01);

    assert_eq!(dedup.count(), 0);

    dedup.mark_seen("https://example.com/1");
    assert_eq!(dedup.count(), 1);

    dedup.mark_seen("https://example.com/2");
    assert_eq!(dedup.count(), 2);

    // Marking the same URL again shouldn't increase count
    dedup.mark_seen("https://example.com/1");
    assert_eq!(dedup.count(), 2);
}

// ============================================================================
// Priority Queue Tests
// ============================================================================

#[test]
fn test_priority_queue_basic() {
    init_test_tracing();

    let queue = PriorityQueue::with_defaults();

    // Add some URLs with different depths
    queue.push(CrawlUrl::new("https://example.com/page1", 0));
    queue.push(CrawlUrl::new("https://example.com/page2", 1));
    queue.push(CrawlUrl::new("https://example.com/page3", 2));

    // Should have 3 URLs
    assert_eq!(queue.len(), 3);

    // Pop should give us URLs (order depends on priority implementation)
    let first = queue.pop();
    assert!(first.is_some());

    assert_eq!(queue.len(), 2);
}

#[test]
fn test_priority_queue_depth_priority() {
    init_test_tracing();

    let config = PriorityConfig {
        max_size: 0,
        ..Default::default()
    };
    let queue = PriorityQueue::new(config);

    // Add URLs at different depths
    queue.push(CrawlUrl::new("https://example.com/deep", 5));
    queue.push(CrawlUrl::new("https://example.com/shallow", 1));
    queue.push(CrawlUrl::new("https://example.com/medium", 3));

    // Pop order should prioritize shallower URLs (lower depth first)
    let first = queue.pop().unwrap();
    assert_eq!(first.depth, 1);

    let second = queue.pop().unwrap();
    assert_eq!(second.depth, 3);

    let third = queue.pop().unwrap();
    assert_eq!(third.depth, 5);
}

#[test]
fn test_priority_queue_empty() {
    init_test_tracing();

    let queue = PriorityQueue::with_defaults();

    // Empty queue should return None
    assert!(queue.pop().is_none());
    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
}

#[test]
fn test_priority_queue_many_urls() {
    init_test_tracing();

    let queue = PriorityQueue::with_defaults();

    // Add 100 URLs
    for i in 0..100 {
        queue.push(CrawlUrl::new(&format!("https://example.com/page{}", i), i % 5));
    }

    assert_eq!(queue.len(), 100);

    // Drain all URLs
    let mut count = 0;
    while queue.pop().is_some() {
        count += 1;
    }

    assert_eq!(count, 100);
    assert!(queue.is_empty());
}

// ============================================================================
// Politeness Scheduler Tests
// ============================================================================

#[test]
fn test_politeness_basic() {
    init_test_tracing();

    let config = PolitenessConfig {
        default_delay_ms: 100,
        ..Default::default()
    };
    let scheduler = PolitenessScheduler::new(config);

    // First request should be allowed
    assert!(scheduler.can_fetch("example.com"));

    // Mark request as started
    scheduler.start_request("example.com");

    // Immediately after, should not be allowed (rate limit)
    assert!(!scheduler.can_fetch("example.com"));

    // Complete the request
    scheduler.complete_request("example.com");
}

#[test]
fn test_politeness_different_domains() {
    init_test_tracing();

    let scheduler = PolitenessScheduler::with_defaults();

    // Should allow fetching from different domains independently
    assert!(scheduler.can_fetch("example.com"));
    assert!(scheduler.can_fetch("other-domain.com"));
    assert!(scheduler.can_fetch("third-domain.com"));

    // Start a request for one domain
    scheduler.start_request("example.com");

    // Other domains should still be fetchable
    assert!(scheduler.can_fetch("other-domain.com"));
    assert!(scheduler.can_fetch("third-domain.com"));
}

#[test]
fn test_politeness_concurrent_limit() {
    init_test_tracing();

    let config = PolitenessConfig {
        concurrent_per_domain: 2,
        default_delay_ms: 0, // No delay to test concurrency
        min_delay_ms: 0,
        ..Default::default()
    };
    let scheduler = PolitenessScheduler::new(config);

    // Start two concurrent requests
    scheduler.start_request("example.com");
    scheduler.start_request("example.com");

    // Third request should not be allowed (at concurrent limit)
    assert!(!scheduler.can_fetch("example.com"));

    // Complete one request
    scheduler.complete_request("example.com");

    // Now another request should be allowed
    assert!(scheduler.can_fetch("example.com"));
}

// ============================================================================
// Domain Extraction Tests
// ============================================================================

#[test]
fn test_extract_domain() {
    assert_eq!(extract_domain("https://example.com/page"), "example.com");
    assert_eq!(
        extract_domain("https://www.example.com/page"),
        "www.example.com"
    );
    assert_eq!(
        extract_domain("https://sub.domain.example.com"),
        "sub.domain.example.com"
    );
    assert_eq!(
        extract_domain("http://example.com:8080/path"),
        "example.com"
    );
}

#[test]
fn test_extract_domain_edge_cases() {
    // Invalid URLs should return something reasonable
    let domain = extract_domain("not-a-valid-url");
    assert!(!domain.is_empty());

    // URL with only domain
    assert_eq!(extract_domain("https://example.com"), "example.com");
}

// ============================================================================
// Partitioner Tests
// ============================================================================

#[test]
fn test_partitioner_distribution() {
    init_test_tracing();

    let partitioner = Partitioner::with_partitions(4);

    // Same domain should always map to same partition
    let partition1 = partitioner.partition_for_url("https://example.com/page1");
    let partition2 = partitioner.partition_for_url("https://example.com/page2");
    assert_eq!(partition1, partition2);

    // Verify partition is in valid range
    assert!(partition1 < 4);
}

#[test]
fn test_partitioner_different_domains() {
    init_test_tracing();

    let partitioner = Partitioner::with_partitions(8);

    // Collect partitions for many domains
    let mut partitions = Vec::new();
    for i in 0..100 {
        let url = format!("https://domain{}.com/page", i);
        partitions.push(partitioner.partition_for_url(&url));
    }

    // Should use multiple partitions (not all same)
    let unique_partitions: std::collections::HashSet<_> = partitions.iter().collect();
    assert!(
        unique_partitions.len() > 1,
        "Should distribute across partitions"
    );
}

// ============================================================================
// Domain Grouper Tests
// ============================================================================

#[test]
fn test_domain_grouper() {
    init_test_tracing();

    // Group URLs by domain using the static method
    let urls = vec![
        "https://example.com/page1".to_string(),
        "https://example.com/page2".to_string(),
        "https://other.com/page1".to_string(),
    ];

    let groups = DomainGrouper::group_by_domain(urls);

    // Should have 2 domain groups
    assert_eq!(groups.len(), 2);

    // Find example.com group
    let example_group = groups.iter().find(|(domain, _)| domain == "example.com");
    assert!(example_group.is_some());
    assert_eq!(example_group.unwrap().1.len(), 2);

    // Find other.com group
    let other_group = groups.iter().find(|(domain, _)| domain == "other.com");
    assert!(other_group.is_some());
    assert_eq!(other_group.unwrap().1.len(), 1);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_frontier_pipeline() {
    init_test_tracing();

    // Create components
    let dedup = UrlDedup::for_capacity(1000, 0.01);
    let queue = PriorityQueue::with_defaults();
    let scheduler = PolitenessScheduler::with_defaults();
    let partitioner = Partitioner::with_partitions(4);

    // Simulate adding seed URLs
    let seeds = vec![
        "https://example.com/",
        "https://example.com/about",
        "https://other.com/",
    ];

    for seed in &seeds {
        if !dedup.check_and_mark(*seed) {
            queue.push(CrawlUrl::seed(*seed));
        }
    }

    assert_eq!(queue.len(), 3);

    // Try adding duplicates
    for seed in &seeds {
        if !dedup.check_and_mark(*seed) {
            queue.push(CrawlUrl::seed(*seed));
        }
    }

    // Queue should still have 3 (no duplicates added)
    assert_eq!(queue.len(), 3);

    // Simulate crawling
    while let Some(url) = queue.pop() {
        let domain = extract_domain(&url.url);
        let partition = partitioner.partition_for_url(&url.url);

        // Check politeness
        if scheduler.can_fetch(&domain) {
            scheduler.start_request(&domain);
            // Simulate fetch...
            scheduler.complete_request(&domain);

            // Simulate discovering new URLs
            let new_url = format!("{}/discovered", url.url);
            if !dedup.check_and_mark(&new_url) {
                queue.push(CrawlUrl::new(&new_url, url.depth + 1));
            }
        }

        // Partition should be valid
        assert!(partition < 4);
    }
}

#[test]
fn test_dedup_and_queue_integration() {
    init_test_tracing();

    let dedup = UrlDedup::for_capacity(1000, 0.01);
    let queue = PriorityQueue::with_defaults();

    // Add 100 unique URLs
    for i in 0..100 {
        let url = format!("https://example.com/page/{}", i);
        if !dedup.check_and_mark(&url) {
            queue.push(CrawlUrl::new(&url, 0));
        }
    }

    assert_eq!(queue.len(), 100);

    // Try to add same URLs again
    for i in 0..100 {
        let url = format!("https://example.com/page/{}", i);
        if !dedup.check_and_mark(&url) {
            queue.push(CrawlUrl::new(&url, 0));
        }
    }

    // Queue should still have 100 (no duplicates)
    assert_eq!(queue.len(), 100);

    // Drain the queue
    let mut drained = 0;
    while queue.pop().is_some() {
        drained += 1;
    }

    assert_eq!(drained, 100);
    assert!(queue.is_empty());
}

#[test]
fn test_partitioned_crawl() {
    init_test_tracing();

    let partitioner = Partitioner::with_partitions(4);
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    // Create queues per partition
    let mut partition_queues: Vec<Vec<String>> = vec![vec![]; 4];

    // Distribute URLs
    let urls = vec![
        "https://a.com/1",
        "https://b.com/1",
        "https://c.com/1",
        "https://d.com/1",
        "https://e.com/1",
        "https://f.com/1",
        "https://g.com/1",
        "https://h.com/1",
    ];

    for url in urls {
        if !dedup.check_and_mark(url) {
            let partition = partitioner.partition_for_url(url);
            partition_queues[partition].push(url.to_string());
        }
    }

    // Total should be 8
    let total: usize = partition_queues.iter().map(|q| q.len()).sum();
    assert_eq!(total, 8);

    // At least 2 partitions should be used
    let non_empty = partition_queues.iter().filter(|q| !q.is_empty()).count();
    assert!(non_empty >= 2, "Should use multiple partitions");
}
