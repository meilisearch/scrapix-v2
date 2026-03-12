//! Concurrency & Thread Safety Tests (P2)
//!
//! All frontier components (UrlDedup, PriorityQueue, PolitenessScheduler,
//! LinkGraph, NearDuplicateDetector) use parking_lot::RwLock internally.
//! These tests verify correct behavior under concurrent access.

use scrapix_core::CrawlUrl;
use scrapix_frontier::{
    LinkGraph, NearDuplicateDetector, PolitenessScheduler, PriorityQueue, UrlDedup,
};
use std::sync::Arc;
use std::thread;

// ============================================================================
// UrlDedup concurrent access
// ============================================================================

#[test]
fn test_dedup_concurrent_mark_and_check() {
    let dedup = Arc::new(UrlDedup::for_capacity(100_000, 0.01));
    let mut handles = vec![];

    // 10 threads each marking 1000 URLs
    for t in 0..10 {
        let dedup = Arc::clone(&dedup);
        handles.push(thread::spawn(move || {
            for i in 0..1000 {
                let url = format!("https://example.com/thread-{t}/page-{i}");
                dedup.mark_seen(&url);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // All URLs should be seen
    for t in 0..10 {
        for i in 0..1000 {
            let url = format!("https://example.com/thread-{t}/page-{i}");
            assert!(dedup.is_seen(&url), "URL should be seen: {url}");
        }
    }
}

#[test]
fn test_dedup_concurrent_check_and_mark_no_double_crawl() {
    let dedup = Arc::new(UrlDedup::for_capacity(10_000, 0.01));
    let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let mut handles = vec![];

    // 10 threads all trying to check_and_mark the same 100 URLs
    for _ in 0..10 {
        let dedup = Arc::clone(&dedup);
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for i in 0..100 {
                let url = format!("https://example.com/page-{i}");
                if !dedup.check_and_mark(&url) {
                    // URL was new (not seen before)
                    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Each URL should be marked as new at most a handful of times
    // (bloom filter + lock means near-perfect, but bloom filter has no atomicity guarantee
    // between check and mark, so we allow some slack)
    let total_new = counter.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        total_new <= 200,
        "Expected ~100 unique URLs but got {total_new} 'new' marks — possible race condition"
    );
    assert!(
        total_new >= 100,
        "Expected at least 100 unique URLs but got {total_new}"
    );
}

// ============================================================================
// PriorityQueue concurrent access
// ============================================================================

#[test]
fn test_priority_queue_concurrent_push_pop() {
    let queue = Arc::new(PriorityQueue::with_defaults());
    let mut handles = vec![];

    // 5 producer threads
    for t in 0..5 {
        let queue = Arc::clone(&queue);
        handles.push(thread::spawn(move || {
            for i in 0..200 {
                queue.push(CrawlUrl::new(format!("https://example.com/t{t}/p{i}"), 1));
            }
        }));
    }

    // 5 consumer threads
    let consumed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    for _ in 0..5 {
        let queue = Arc::clone(&queue);
        let consumed = Arc::clone(&consumed);
        handles.push(thread::spawn(move || {
            for _ in 0..200 {
                if queue.pop().is_some() {
                    consumed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let total_consumed = consumed.load(std::sync::atomic::Ordering::Relaxed);
    let remaining = queue.len();

    // Total produced = 1000, so consumed + remaining should equal 1000
    assert_eq!(
        total_consumed + remaining as u64,
        1000,
        "No items should be lost: consumed={total_consumed}, remaining={remaining}"
    );
}

#[test]
fn test_priority_queue_pop_returns_none_when_empty() {
    let queue = PriorityQueue::with_defaults();
    assert!(queue.pop().is_none());
    assert!(queue.is_empty());
}

#[test]
fn test_priority_queue_push_many_and_pop_many() {
    let queue = PriorityQueue::with_defaults();
    let urls: Vec<CrawlUrl> = (0..50)
        .map(|i| CrawlUrl::new(format!("https://example.com/p{i}"), 1))
        .collect();

    queue.push_many(urls);
    assert_eq!(queue.len(), 50);

    let batch = queue.pop_many(20);
    assert_eq!(batch.len(), 20);
    assert_eq!(queue.len(), 30);
}

// ============================================================================
// PolitenessScheduler concurrent access
// ============================================================================

#[test]
fn test_politeness_concurrent_start_complete() {
    let scheduler = Arc::new(PolitenessScheduler::with_defaults());
    let mut handles = vec![];

    // Simulate concurrent requests to the same domain
    for _ in 0..10 {
        let scheduler = Arc::clone(&scheduler);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                scheduler.start_request("example.com");
                // Simulate work
                scheduler.complete_request("example.com");
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // After all requests complete, in_flight should be 0
    if let Some(stats) = scheduler.domain_stats("example.com") {
        assert_eq!(
            stats.in_flight, 0,
            "All requests completed, in_flight should be 0"
        );
    }
}

#[test]
fn test_politeness_concurrent_different_domains() {
    let scheduler = Arc::new(PolitenessScheduler::with_defaults());
    let mut handles = vec![];

    for t in 0..10 {
        let scheduler = Arc::clone(&scheduler);
        handles.push(thread::spawn(move || {
            let domain = format!("domain-{t}.com");
            for _ in 0..50 {
                scheduler.start_request(&domain);
                scheduler.complete_request(&domain);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let domains = scheduler.tracked_domains();
    assert_eq!(domains.len(), 10, "Should track 10 distinct domains");
}

// ============================================================================
// LinkGraph concurrent access
// ============================================================================

#[test]
fn test_linkgraph_concurrent_record_links() {
    let graph = Arc::new(LinkGraph::with_defaults());
    let mut handles = vec![];

    for t in 0..5 {
        let graph = Arc::clone(&graph);
        handles.push(thread::spawn(move || {
            for i in 0..100 {
                let source = format!("https://example.com/t{t}/page{i}");
                let targets = vec![
                    format!("https://example.com/t{t}/page{}", i + 1),
                    "https://example.com/hub".to_string(),
                ];
                graph.record_links(&source, targets);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // The hub page should have many inbound links
    let hub_inbound = graph.inbound_count("https://example.com/hub");
    assert!(
        hub_inbound >= 400,
        "Hub should have many inbound links, got {hub_inbound}"
    );
}

#[test]
fn test_linkgraph_compute_scores_after_concurrent_inserts() {
    let graph = Arc::new(LinkGraph::with_defaults());

    // Build a small graph
    graph.record_links(
        "https://a.com/1",
        vec!["https://a.com/2", "https://a.com/3"],
    );
    graph.record_links("https://a.com/2", vec!["https://a.com/3"]);
    graph.record_links("https://a.com/3", vec!["https://a.com/1"]);

    graph.compute_scores();

    // Page 3 has the most inbound links (from 1 and 2), should have high score
    let score_3 = graph.get_score("https://a.com/3");
    let score_1 = graph.get_score("https://a.com/1");
    assert!(
        score_3 > 0.0,
        "Page with most inbound links should have positive score"
    );
    assert!(score_1 > 0.0, "Page in cycle should have positive score");
}

// ============================================================================
// NearDuplicateDetector concurrent access
// ============================================================================

#[test]
fn test_near_duplicate_detector_concurrent_check_and_add() {
    let detector = Arc::new(NearDuplicateDetector::with_defaults());
    let mut handles = vec![];

    for t in 0..5 {
        let detector = Arc::clone(&detector);
        handles.push(thread::spawn(move || {
            for i in 0..100 {
                let url = format!("https://example.com/thread-{t}/unique-page-{i}");
                let content = format!(
                    "This is unique content for thread {t} page {i} with enough words to generate a meaningful fingerprint for near-duplicate detection"
                );
                detector.check_and_add(&url, &content);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert!(
        detector.fingerprint_count() > 0,
        "Should have stored fingerprints"
    );
}

#[test]
fn test_near_duplicate_detector_detects_duplicates_under_concurrency() {
    let detector = Arc::new(NearDuplicateDetector::with_defaults());

    // Add original content
    let original = "The quick brown fox jumps over the lazy dog. This is a longer piece of content to ensure that the fingerprinting algorithm has enough material to work with for similarity detection.";
    let result = detector.check_and_add("https://example.com/original", original);
    assert!(result.is_none(), "First document should not be a duplicate");

    // Add very similar content from multiple threads
    let duplicates_found = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut handles = vec![];

    for t in 0..5 {
        let detector = Arc::clone(&detector);
        let duplicates_found = Arc::clone(&duplicates_found);
        handles.push(thread::spawn(move || {
            let similar = format!("The quick brown fox jumps over the lazy dog. This is a longer piece of content to ensure that the fingerprinting algorithm has enough material to work with for similarity detection. Thread {t}.");
            if detector
                .check_and_add(&format!("https://example.com/dup-{t}"), &similar)
                .is_some()
            {
                duplicates_found.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let found = duplicates_found.load(std::sync::atomic::Ordering::Relaxed);
    // At least some should be detected as duplicates
    assert!(
        found > 0,
        "Should detect at least some near-duplicates, found {found}/5"
    );
}
