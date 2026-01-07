//! End-to-end integration tests for the Scrapix pipeline
//!
//! These tests verify the complete crawl -> process -> store pipeline works correctly
//! by simulating the full flow with mock services.

use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use scrapix_core::{CrawlUrl, RawPage};
use scrapix_crawler::extractor::{ExtractorConfig, UrlExtractor};
use scrapix_frontier::{
    NearDuplicateConfig, NearDuplicateDetector, PolitenessConfig, PolitenessScheduler,
    PriorityQueue, RecrawlConfig, RecrawlScheduler, SimHash, UrlDedup, UrlHistory,
    UrlHistoryConfig,
};
use scrapix_parser::HtmlParser;

use crate::{create_raw_page, fixtures, init_test_tracing};

/// Metrics collector for e2e tests
#[derive(Debug, Default)]
struct TestMetrics {
    pages_fetched: AtomicU64,
    pages_processed: AtomicU64,
    urls_discovered: AtomicU64,
    urls_deduplicated: AtomicU64,
    errors: AtomicU64,
    near_duplicates_detected: AtomicU64,
}

impl TestMetrics {
    fn new() -> Self {
        Self::default()
    }

    fn record_fetch(&self) {
        self.pages_fetched.fetch_add(1, Ordering::Relaxed);
    }

    fn record_process(&self) {
        self.pages_processed.fetch_add(1, Ordering::Relaxed);
    }

    fn record_urls_discovered(&self, count: u64) {
        self.urls_discovered.fetch_add(count, Ordering::Relaxed);
    }

    fn record_dedup(&self) {
        self.urls_deduplicated.fetch_add(1, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    fn record_near_duplicate(&self) {
        self.near_duplicates_detected.fetch_add(1, Ordering::Relaxed);
    }

    fn summary(&self) -> String {
        format!(
            "Pages fetched: {}, Processed: {}, URLs discovered: {}, Deduped: {}, Near-dupes: {}, Errors: {}",
            self.pages_fetched.load(Ordering::Relaxed),
            self.pages_processed.load(Ordering::Relaxed),
            self.urls_discovered.load(Ordering::Relaxed),
            self.urls_deduplicated.load(Ordering::Relaxed),
            self.near_duplicates_detected.load(Ordering::Relaxed),
            self.errors.load(Ordering::Relaxed),
        )
    }
}

/// Mock HTTP server that serves pre-defined pages
struct MockWebServer {
    pages: HashMap<String, String>,
    request_count: AtomicU64,
    delay_ms: u64,
}

impl MockWebServer {
    fn new() -> Self {
        let mut pages = HashMap::new();

        // Add test pages
        pages.insert("/".to_string(), fixtures::SIMPLE_ARTICLE.to_string());
        pages.insert("/page1".to_string(), fixtures::PAGE_WITH_LINKS.to_string());
        pages.insert("/page2".to_string(), fixtures::PAGE_WITH_SCHEMA.to_string());
        pages.insert("/page3".to_string(), fixtures::FRENCH_PAGE.to_string());
        pages.insert("/about".to_string(), fixtures::MINIMAL_PAGE.to_string());
        pages.insert("/contact".to_string(), fixtures::MINIMAL_PAGE.to_string());

        // Add duplicate content pages (for near-duplicate detection)
        pages.insert(
            "/article/1".to_string(),
            fixtures::SIMPLE_ARTICLE.to_string(),
        );
        pages.insert(
            "/article/2".to_string(),
            fixtures::SIMPLE_ARTICLE
                .replace("Test Article Title", "Test Article Title Copy")
                .to_string(),
        );

        Self {
            pages,
            request_count: AtomicU64::new(0),
            delay_ms: 10,
        }
    }

    fn add_page(&mut self, path: &str, html: &str) {
        self.pages.insert(path.to_string(), html.to_string());
    }

    async fn fetch(&self, url: &str) -> Option<RawPage> {
        self.request_count.fetch_add(1, Ordering::Relaxed);

        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;

        // Parse URL to get path
        let parsed = url::Url::parse(url).ok()?;
        let path = parsed.path();

        self.pages.get(path).map(|html| RawPage {
            url: url.to_string(),
            final_url: url.to_string(),
            status: 200,
            headers: HashMap::new(),
            html: html.clone(),
            content_type: Some("text/html; charset=utf-8".to_string()),
            js_rendered: false,
            fetched_at: Utc::now(),
            fetch_duration_ms: self.delay_ms as u64,
        })
    }

    fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }
}

/// Simulated message queue for testing pipeline flow
struct TestMessageQueue {
    crawl_queue: mpsc::Sender<CrawlUrl>,
    crawl_receiver: Mutex<Option<mpsc::Receiver<CrawlUrl>>>,
}

impl TestMessageQueue {
    fn new(buffer_size: usize) -> Self {
        let (crawl_tx, crawl_rx) = mpsc::channel(buffer_size);

        Self {
            crawl_queue: crawl_tx,
            crawl_receiver: Mutex::new(Some(crawl_rx)),
        }
    }

    #[allow(dead_code)]
    async fn send_crawl_url(&self, url: CrawlUrl) -> Result<(), &'static str> {
        self.crawl_queue
            .send(url)
            .await
            .map_err(|_| "Failed to send to crawl queue")
    }

    #[allow(dead_code)]
    fn take_crawl_receiver(&self) -> Option<mpsc::Receiver<CrawlUrl>> {
        self.crawl_receiver.lock().take()
    }
}

/// Test the complete crawl -> URL extraction -> dedup -> queue pipeline
#[tokio::test]
async fn test_full_crawl_pipeline() {
    init_test_tracing();

    let metrics = Arc::new(TestMetrics::new());
    let server = Arc::new(MockWebServer::new());
    let _queue = Arc::new(TestMessageQueue::new(1000));

    // Initialize frontier components
    let dedup = Arc::new(UrlDedup::for_capacity(10000, 0.01));
    let priority_queue = Arc::new(PriorityQueue::with_defaults());
    let politeness = Arc::new(PolitenessScheduler::new(PolitenessConfig {
        default_delay_ms: 10,
        concurrent_per_domain: 2,
        ..Default::default()
    }));

    // Initialize URL extractor
    let extractor = UrlExtractor::new(ExtractorConfig {
        max_depth: 3,
        follow_external: false,
        follow_subdomains: true,
        ..Default::default()
    });

    // Seed URL
    let seed_url = CrawlUrl::seed("https://example.com/");
    dedup.check_and_mark(&seed_url.url);
    priority_queue.push(seed_url);

    let domain = "example.com";

    // Simulate crawl loop (process up to 10 URLs)
    let mut processed = 0;
    let max_urls = 10;

    while processed < max_urls {
        // Get next URL from priority queue
        let url = match priority_queue.pop() {
            Some(u) => u,
            None => break,
        };

        // Check politeness
        if !politeness.can_fetch(domain) {
            priority_queue.push(url);
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }

        // Fetch page
        politeness.start_request(domain);
        let page = server.fetch(&url.url).await;
        politeness.complete_request(domain);

        let page = match page {
            Some(p) => p,
            None => {
                metrics.record_error();
                continue;
            }
        };

        metrics.record_fetch();

        // Extract URLs
        let extracted_urls = extractor.extract(&page, url.depth);
        metrics.record_urls_discovered(extracted_urls.len() as u64);

        // Deduplicate and add to queue
        for extracted in extracted_urls {
            if !dedup.check_and_mark(&extracted.url) {
                priority_queue.push(extracted);
            } else {
                metrics.record_dedup();
            }
        }

        processed += 1;
    }

    // Verify results
    assert!(metrics.pages_fetched.load(Ordering::Relaxed) > 0, "Should have fetched pages");
    assert!(
        metrics.urls_discovered.load(Ordering::Relaxed) > 0,
        "Should have discovered URLs"
    );
    assert!(
        server.request_count() <= max_urls as u64,
        "Should respect max URL limit"
    );

    println!("Pipeline test results: {}", metrics.summary());
}

/// Test near-duplicate detection in the pipeline
#[tokio::test]
async fn test_near_duplicate_detection_pipeline() {
    init_test_tracing();

    let detector = NearDuplicateDetector::new(NearDuplicateConfig {
        use_simhash: true,
        simhash_threshold: 10,
        ..Default::default()
    });

    // Original document
    let doc1_content = r#"
        <html>
        <head><title>Original Article</title></head>
        <body>
            <article>
                <h1>Important News About Technology</h1>
                <p>This is a comprehensive article about the latest developments in technology.
                We cover artificial intelligence, machine learning, and cloud computing.
                The industry is rapidly evolving with new innovations every day.</p>
                <p>Experts predict significant growth in the coming years as companies
                invest more in digital transformation and automation technologies.</p>
            </article>
        </body>
        </html>
    "#;

    // Near-duplicate (slightly modified)
    let doc2_content = r#"
        <html>
        <head><title>Original Article Copy</title></head>
        <body>
            <article>
                <h1>Important News About Technology</h1>
                <p>This is a comprehensive article about the latest developments in technology.
                We cover artificial intelligence, machine learning, and cloud computing.
                The industry is rapidly evolving with new innovations every day.</p>
                <p>Experts predict significant growth in the coming years as companies
                invest more in digital transformation and automation technologies.</p>
            </article>
        </body>
        </html>
    "#;

    // Completely different document
    let doc3_content = r#"
        <html>
        <head><title>Recipe Blog</title></head>
        <body>
            <article>
                <h1>Delicious Chocolate Cake Recipe</h1>
                <p>Learn how to make the most delicious chocolate cake with this easy recipe.
                You will need flour, sugar, cocoa powder, eggs, and butter.</p>
                <p>Preheat your oven to 350 degrees and mix all ingredients together.
                Bake for 30 minutes until a toothpick comes out clean.</p>
            </article>
        </body>
        </html>
    "#;

    // Process documents
    let result1 = detector.check_and_add("https://example.com/article1", doc1_content);
    assert!(result1.is_none(), "First document should not be a duplicate");

    let result2 = detector.check_and_add("https://example.com/article2", doc2_content);
    assert!(
        result2.is_some(),
        "Second document should be detected as near-duplicate"
    );

    let result3 = detector.check_and_add("https://example.com/recipe", doc3_content);
    assert!(
        result3.is_none(),
        "Different document should not be a duplicate"
    );

    let stats = detector.stats();
    assert_eq!(stats.documents_checked, 3);
    assert_eq!(stats.duplicates_found, 1);
    assert_eq!(stats.unique_documents, 2);

    println!(
        "Near-duplicate detection: {} checked, {} unique, {} duplicates",
        stats.documents_checked, stats.unique_documents, stats.duplicates_found
    );
}

/// Test incremental crawling with history tracking
#[tokio::test]
async fn test_incremental_crawl_pipeline() {
    init_test_tracing();

    // Note: RecrawlScheduler uses second-level granularity (as_secs()),
    // so we need to use at least 1 second intervals for proper testing.
    let history = Arc::new(UrlHistory::new(UrlHistoryConfig {
        default_recrawl_interval: Duration::from_secs(1),
        min_recrawl_interval: Duration::from_secs(1),
        max_recrawl_interval: Duration::from_secs(10),
        ..Default::default()
    }));

    let scheduler = RecrawlScheduler::new(
        RecrawlConfig {
            enabled: true,
            min_age: Duration::from_secs(1),
            max_age: Duration::from_secs(10),
            ..Default::default()
        },
        history.clone(),
    );

    let url = "https://example.com/page";

    // First crawl - should always crawl
    let decision1 = scheduler.should_crawl(&CrawlUrl::seed(url));
    assert!(
        matches!(
            decision1,
            scrapix_frontier::RecrawlDecision::Crawl { .. }
        ),
        "First crawl should proceed"
    );

    // Record the crawl
    let record = scrapix_frontier::history::CrawlRecord::new()
        .with_content_hash("hash_v1")
        .with_etag("\"etag1\"");
    history.record_crawl(url, record);

    // Immediate re-check - should skip (too recent, less than 1 second old)
    let decision2 = scheduler.should_crawl(&CrawlUrl::seed(url));
    assert!(
        matches!(
            decision2,
            scrapix_frontier::RecrawlDecision::Skip { .. }
        ),
        "Should skip immediate re-crawl"
    );

    // Wait for interval to elapse (1 second + buffer)
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // Now should crawl again
    let decision3 = scheduler.should_crawl(&CrawlUrl::seed(url));
    assert!(
        matches!(
            decision3,
            scrapix_frontier::RecrawlDecision::Crawl { .. }
        ),
        "Should crawl after interval"
    );

    // Record with new content (content changed)
    let record2 = scrapix_frontier::history::CrawlRecord::new()
        .with_content_hash("hash_v2")
        .with_etag("\"etag2\"");
    history.record_crawl(url, record2);

    // Check that content change was detected
    let stored = history.get_record(url).unwrap();
    assert!(stored.crawl_count >= 2, "Should have recorded multiple crawls");

    println!(
        "Incremental crawl test: {} crawls, change rate: {:.2}",
        stored.crawl_count,
        stored.change_rate()
    );
}

/// Test document parsing pipeline
#[tokio::test]
async fn test_document_parsing_pipeline() {
    init_test_tracing();

    let parser = HtmlParser::with_defaults();

    let pages = vec![
        ("https://example.com/article", fixtures::SIMPLE_ARTICLE),
        ("https://example.com/product", fixtures::PAGE_WITH_SCHEMA),
        ("https://example.com/french", fixtures::FRENCH_PAGE),
    ];

    let mut results = Vec::new();

    for (url, html) in pages {
        let page = create_raw_page(url, html);
        let doc = parser.parse(&page).expect("Failed to parse page");

        results.push((url, doc));
    }

    // Verify article parsing
    let (url, doc) = &results[0];
    assert!(doc.title.is_some(), "Article should have title");
    assert!(doc.content.is_some(), "Article should have content");
    // Description is stored in metadata
    assert!(
        doc.metadata.as_ref().map(|m| m.contains_key("description")).unwrap_or(false),
        "Article should have description in metadata"
    );
    println!("Parsed {}: {:?}", url, doc.title);

    // Verify product page parsing (should extract structured data)
    let (url, doc) = &results[1];
    assert!(doc.title.is_some(), "Product page should have title");
    println!("Parsed {}: {:?}", url, doc.title);

    // Verify French page parsing
    let (url, doc) = &results[2];
    assert!(doc.title.is_some(), "French page should have title");
    // Language detection should identify French
    assert!(
        doc.language.is_some() && doc.language.as_ref().unwrap().contains("fr"),
        "Should detect French language"
    );
    println!(
        "Parsed {}: {:?}, lang: {:?}",
        url, doc.title, doc.language
    );

    println!("Document parsing pipeline: {} documents processed", results.len());
}

/// Test concurrent crawling simulation
#[tokio::test]
async fn test_concurrent_crawl_simulation() {
    init_test_tracing();

    let metrics = Arc::new(TestMetrics::new());
    let server = Arc::new(MockWebServer::new());
    let dedup = Arc::new(UrlDedup::for_capacity(10000, 0.01));
    let processed_urls = Arc::new(Mutex::new(Vec::new()));

    // Generate test URLs
    let urls: Vec<CrawlUrl> = (0..20)
        .map(|i| {
            let path = match i % 5 {
                0 => "/",
                1 => "/page1",
                2 => "/page2",
                3 => "/about",
                _ => "/contact",
            };
            CrawlUrl::seed(&format!("https://example.com{}", path))
        })
        .collect();

    // Simulate concurrent workers
    let num_workers = 4;
    let mut handles = Vec::new();

    let urls_arc = Arc::new(Mutex::new(urls));

    for worker_id in 0..num_workers {
        let server = server.clone();
        let metrics = metrics.clone();
        let dedup = dedup.clone();
        let urls = urls_arc.clone();
        let processed = processed_urls.clone();

        handles.push(tokio::spawn(async move {
            loop {
                // Get next URL
                let url = {
                    let mut queue = urls.lock();
                    queue.pop()
                };

                let url = match url {
                    Some(u) => u,
                    None => break,
                };

                // Check dedup
                if dedup.check_and_mark(&url.url) {
                    metrics.record_dedup();
                    continue;
                }

                // Fetch
                if let Some(_page) = server.fetch(&url.url).await {
                    metrics.record_fetch();
                    processed.lock().push((worker_id, url.url.clone()));
                } else {
                    metrics.record_error();
                }
            }
        }));
    }

    // Wait for all workers
    for handle in handles {
        handle.await.unwrap();
    }

    let processed_count = processed_urls.lock().len();
    let fetched = metrics.pages_fetched.load(Ordering::Relaxed);
    let deduped = metrics.urls_deduplicated.load(Ordering::Relaxed);

    println!(
        "Concurrent crawl: {} workers, {} fetched, {} deduped, {} total processed",
        num_workers, fetched, deduped, processed_count
    );

    // Verify deduplication worked (should have fewer fetches than total URLs due to duplicates)
    assert!(
        fetched + deduped == 20,
        "Total should equal fetched + deduped"
    );
    assert!(fetched <= 5, "Should have at most 5 unique URLs");
}

/// Test SimHash fingerprinting
#[tokio::test]
async fn test_simhash_fingerprinting() {
    let simhash = SimHash::new();

    // Test with HTML content
    let html1 = fixtures::SIMPLE_ARTICLE;
    let html2 = fixtures::SIMPLE_ARTICLE.replace("Test", "Example");
    let html3 = fixtures::FRENCH_PAGE;

    let hash1 = simhash.hash(html1);
    let hash2 = simhash.hash(&html2);
    let hash3 = simhash.hash(html3);

    let dist12 = SimHash::hamming_distance(hash1, hash2);
    let dist13 = SimHash::hamming_distance(hash1, hash3);

    println!(
        "SimHash distances: similar={}, different={}",
        dist12, dist13
    );

    // Similar content should have smaller distance
    assert!(
        dist12 < dist13,
        "Similar content should have smaller Hamming distance"
    );
}

/// Test rate limiting and politeness
#[tokio::test]
async fn test_rate_limiting_pipeline() {
    init_test_tracing();

    let scheduler = PolitenessScheduler::new(PolitenessConfig {
        default_delay_ms: 50,
        concurrent_per_domain: 1,
        ..Default::default()
    });

    let domain = "example.com";
    let start = std::time::Instant::now();

    // Make multiple requests
    let mut request_count = 0;

    for _ in 0..5 {
        // Wait until we can fetch
        while !scheduler.can_fetch(domain) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        scheduler.start_request(domain);
        request_count += 1;

        // Simulate request
        tokio::time::sleep(Duration::from_millis(10)).await;

        scheduler.complete_request(domain);
    }

    // Verify rate limiting
    let total_time = start.elapsed();
    println!(
        "Rate limiting test: {} requests in {:?}",
        request_count, total_time
    );

    // Should take at least 150ms due to 50ms delay between requests (after initial)
    assert!(
        total_time >= Duration::from_millis(150),
        "Rate limiting should enforce delays"
    );

    assert_eq!(request_count, 5, "Should have made 5 requests");
}
