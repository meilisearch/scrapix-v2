//! Error Resilience Tests (P1)
//!
//! Tests that parsers, extractors, and frontier components handle
//! malformed, empty, huge, and pathological inputs without panicking.

use scrapix_core::{Document, RawPage};
use scrapix_extractor::{BlockSplitter, Extractor, MetadataExtractor, SchemaExtractor};
use scrapix_frontier::{NearDuplicateDetector, PolitenessScheduler, PriorityQueue, UrlDedup};
use scrapix_parser::HtmlParser;
use std::collections::HashMap;

fn make_page(url: &str, html: &str) -> RawPage {
    RawPage {
        url: url.to_string(),
        final_url: url.to_string(),
        status: 200,
        headers: HashMap::new(),
        html: html.to_string(),
        content_type: Some("text/html".to_string()),
        js_rendered: false,
        fetched_at: chrono::Utc::now(),
        fetch_duration_ms: 100,
    }
}

// ============================================================================
// Parser resilience
// ============================================================================

#[test]
fn test_parser_handles_empty_html() {
    let parser = HtmlParser::with_defaults();
    let page = make_page("https://example.com", "");

    // Should either succeed with minimal doc or return an error — not panic
    let result = parser.parse(&page);
    // Both Ok and Err are acceptable, just no panic
    if let Ok(doc) = result {
        assert_eq!(doc.url, "https://example.com");
    }
}

#[test]
fn test_parser_handles_whitespace_only_html() {
    let parser = HtmlParser::with_defaults();
    let page = make_page("https://example.com", "   \n\t  \n  ");

    let result = parser.parse(&page);
    if let Ok(doc) = result {
        assert_eq!(doc.url, "https://example.com");
    }
}

#[test]
fn test_parser_handles_html_without_body() {
    let parser = HtmlParser::with_defaults();
    let page = make_page(
        "https://example.com",
        "<html><head><title>No Body</title></head></html>",
    );

    let result = parser.parse(&page);
    if let Ok(doc) = result {
        assert_eq!(doc.title, Some("No Body".to_string()));
    }
}

#[test]
fn test_parser_handles_html_with_only_scripts() {
    let parser = HtmlParser::with_defaults();
    let html = r#"
        <html>
        <head><title>Script Page</title></head>
        <body>
            <script>console.log("hello");</script>
            <script>var x = 1;</script>
            <noscript>Enable JavaScript</noscript>
        </body>
        </html>
    "#;

    let page = make_page("https://example.com", html);
    let result = parser.parse(&page);

    if let Ok(doc) = result {
        // Content should not include script text
        if let Some(ref content) = doc.content {
            assert!(
                !content.contains("console.log"),
                "Script content should be stripped"
            );
        }
    }
}

#[test]
fn test_parser_handles_malformed_html() {
    let parser = HtmlParser::with_defaults();
    let html = r#"
        <html>
        <body>
            <p>Unclosed paragraph
            <div>
                <span>Mismatched tags</p></div>
            </span>
            <p>More content after mess</p>
        </body>
    "#;

    let page = make_page("https://example.com", html);
    let result = parser.parse(&page);

    // Should not panic, should extract what it can
    if let Ok(doc) = result {
        assert_eq!(doc.url, "https://example.com");
    }
}

#[test]
fn test_parser_handles_null_bytes_in_html() {
    let parser = HtmlParser::with_defaults();
    let html = "<html><body><p>Content with \0 null \0 bytes</p></body></html>";

    let page = make_page("https://example.com", html);
    let result = parser.parse(&page);
    if let Ok(doc) = result {
        assert_eq!(doc.url, "https://example.com");
    }
}

#[test]
fn test_parser_handles_deeply_nested_html() {
    let parser = HtmlParser::with_defaults();

    // 200 levels of nesting
    let mut html = String::new();
    for _ in 0..200 {
        html.push_str("<div>");
    }
    html.push_str("<p>Deep content</p>");
    for _ in 0..200 {
        html.push_str("</div>");
    }

    let page = make_page("https://example.com", &html);
    let result = parser.parse(&page);
    if let Ok(doc) = result {
        assert_eq!(doc.url, "https://example.com");
    }
}

#[test]
fn test_parser_handles_huge_text_content() {
    let parser = HtmlParser::with_defaults();
    let big_content = "word ".repeat(100_000); // ~500KB of text
    let html = format!(
        "<html><body><article><p>{}</p></article></body></html>",
        big_content
    );

    let page = make_page("https://example.com", &html);
    let result = parser.parse(&page);
    if let Ok(doc) = result {
        assert!(doc.content.is_some());
    }
}

#[test]
fn test_parser_extracts_links_from_empty_page() {
    let parser = HtmlParser::with_defaults();
    let page = make_page("https://example.com", "<html><body></body></html>");

    let links = parser.extract_links(&page);
    assert!(links.is_empty());
}

// ============================================================================
// Extractor resilience
// ============================================================================

#[test]
fn test_metadata_extractor_handles_empty_html() {
    let extractor = MetadataExtractor::new();
    let result = extractor.extract("");
    // Should not panic — either Ok or Err is acceptable
    let _ = result;
}

#[test]
fn test_schema_extractor_handles_invalid_json_ld() {
    let extractor = SchemaExtractor::default();
    let html = r#"
        <html>
        <head>
            <script type="application/ld+json">
                {this is not valid JSON}
            </script>
        </head>
        <body><p>Content</p></body>
        </html>
    "#;

    let result = extractor.extract(html);
    // Should not panic
    let _ = result;
}

#[test]
fn test_schema_extractor_handles_empty_json_ld() {
    let extractor = SchemaExtractor::default();
    let html = r#"
        <html>
        <head>
            <script type="application/ld+json"></script>
        </head>
        <body><p>Content</p></body>
        </html>
    "#;

    let result = extractor.extract(html);
    // Should not panic
    let _ = result;
}

#[test]
fn test_full_extractor_handles_empty_html() {
    let extractor = Extractor::with_all_features();
    let result = extractor.extract("");
    // Should return a result without panicking
    let _ = result;
}

#[test]
fn test_block_splitter_handles_no_headings() {
    let splitter = BlockSplitter::with_defaults();
    let html = "<html><body><p>Just a paragraph without any headings at all.</p><p>Another paragraph here.</p></body></html>";

    let result = splitter.split(html);
    // Should not panic
    if let Ok(blocks) = result {
        assert!(blocks.count >= 1 || blocks.blocks.is_empty());
    }
}

#[test]
fn test_block_splitter_handles_many_headings() {
    let mut html = String::from("<html><body><article>");
    for i in 0..100 {
        html.push_str(&format!(
            "<h2>Section {i}</h2><p>Content for section {i}. This is a paragraph with enough text to be meaningful content for splitting purposes.</p>"
        ));
    }
    html.push_str("</article></body></html>");

    let splitter = BlockSplitter::with_defaults();
    let result = splitter.split(&html);
    // Should not panic regardless of number of headings — may or may not produce blocks
    // depending on content extraction heuristics
    let _ = result;
}

// ============================================================================
// Frontier resilience
// ============================================================================

#[test]
fn test_dedup_handles_empty_string() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);
    dedup.mark_seen("");
    assert!(dedup.is_seen(""));
    assert_eq!(dedup.count(), 1);
}

#[test]
fn test_dedup_handles_very_long_url() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);
    let long_url = format!("https://example.com/{}", "a".repeat(10_000));

    dedup.mark_seen(&long_url);
    assert!(dedup.is_seen(&long_url));
}

#[test]
fn test_dedup_handles_unicode_urls() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    dedup.mark_seen("https://example.com/日本語/ページ");
    assert!(dedup.is_seen("https://example.com/日本語/ページ"));

    dedup.mark_seen("https://example.com/путь/страница");
    assert!(dedup.is_seen("https://example.com/путь/страница"));
}

#[test]
fn test_dedup_batch_operations_with_duplicates() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    let urls = vec![
        "https://example.com/1".to_string(),
        "https://example.com/2".to_string(),
        "https://example.com/1".to_string(), // duplicate
        "https://example.com/3".to_string(),
        "https://example.com/2".to_string(), // duplicate
    ];

    dedup.mark_seen_batch(&urls);
    assert_eq!(dedup.count(), 3); // only 3 unique
}

#[test]
fn test_priority_queue_push_pop_order() {
    let queue = PriorityQueue::with_defaults();

    // Higher priority items should come out first
    let url_low = scrapix_core::CrawlUrl::new("https://example.com/low", 5);
    let url_high = scrapix_core::CrawlUrl::new("https://example.com/high", 0);

    queue.push(url_low);
    queue.push(url_high);

    // Depth 0 has higher priority than depth 5
    let first = queue.pop().expect("should have item");
    assert_eq!(first.depth, 0);
}

#[test]
fn test_priority_queue_empty_pop() {
    let queue = PriorityQueue::with_defaults();
    assert!(queue.pop().is_none());
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_near_duplicate_detector_handles_empty_content() {
    let detector = NearDuplicateDetector::with_defaults();
    let result = detector.check_and_add("https://example.com/empty", "");
    // Should not panic — first add should not find a duplicate
    assert!(result.is_none(), "First add should not be a duplicate");
}

#[test]
fn test_near_duplicate_detector_identical_content_different_urls() {
    let detector = NearDuplicateDetector::with_defaults();
    let content = "This is a reasonably long piece of content that should be detected as a near duplicate when it appears on two different URLs with the same text content.";

    let is_dup1 = detector.check_and_add("https://example.com/page1", content);
    assert!(is_dup1.is_none(), "First page should not be a duplicate");

    let is_dup2 = detector.check_and_add("https://example.com/page2", content);
    assert!(
        is_dup2.is_some(),
        "Identical content on different URL should be detected as duplicate"
    );
}

#[test]
fn test_politeness_scheduler_handles_unknown_domain() {
    let scheduler = PolitenessScheduler::with_defaults();
    let can_fetch = scheduler.can_fetch("never-seen-before.example.com");
    // Should allow first request to unknown domain
    assert!(can_fetch);
}

#[test]
fn test_dedup_clear_resets_state() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    dedup.mark_seen("https://example.com/page");
    assert!(dedup.is_seen("https://example.com/page"));
    assert_eq!(dedup.count(), 1);

    dedup.clear();
    assert!(!dedup.is_seen("https://example.com/page"));
    assert_eq!(dedup.count(), 0);
}

#[test]
fn test_dedup_stats_accurate() {
    let dedup = UrlDedup::for_capacity(10_000, 0.01);

    for i in 0..100 {
        dedup.mark_seen(&format!("https://example.com/page/{i}"));
    }

    let stats = dedup.stats();
    assert_eq!(stats.items_count, 100);
    assert_eq!(stats.expected_capacity, 10_000);
    assert!(stats.bitmap_bits > 0);
    assert!(stats.hash_functions > 0);
    assert!(stats.estimated_memory_bytes > 0);
}

// ============================================================================
// Document edge cases
// ============================================================================

#[test]
fn test_document_uid_deterministic() {
    let doc1 = Document::new("https://example.com/page", "example.com");
    let doc2 = Document::new("https://example.com/page", "example.com");

    // Same URL produces the same deterministic UID (UUIDv5)
    assert_eq!(doc1.uid, doc2.uid, "Same URL must produce the same UID");

    // Different URLs produce different UIDs
    let doc3 = Document::new("https://example.com/other", "example.com");
    assert_ne!(
        doc1.uid, doc3.uid,
        "Different URLs must produce different UIDs"
    );
}

#[test]
fn test_document_block_inherits_parent_fields() {
    let mut parent = Document::new("https://example.com/page", "example.com");
    parent.title = Some("Parent Title".to_string());
    parent.language = Some("en".to_string());
    parent.urls_tags = Some(vec!["docs".to_string(), "api".to_string()]);

    let mut metadata = HashMap::new();
    metadata.insert("key".to_string(), "value".to_string());
    parent.metadata = Some(metadata);

    let block = Document::new_block(&parent, 0);

    assert_eq!(block.title, parent.title);
    assert_eq!(block.language, parent.language);
    assert_eq!(block.urls_tags, parent.urls_tags);
    assert_eq!(block.metadata, parent.metadata);
    assert_eq!(block.domain, parent.domain);
    assert_eq!(block.url, parent.url);
    assert!(block.is_block());
    assert_eq!(block.parent_document_id, Some(parent.uid.clone()));
    assert_eq!(block.page_block, Some(0));

    // Block should NOT inherit these
    assert!(block.content.is_none());
    assert!(block.markdown.is_none());
    assert!(block.schema.is_none());
    assert!(block.custom.is_none());
}

#[test]
fn test_document_serialization_skips_none_fields() {
    let doc = Document::new("https://example.com", "example.com");
    let json = serde_json::to_string(&doc).expect("serialize");

    // Fields with skip_serializing_if = "Option::is_none" should be absent
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object().unwrap();

    assert!(obj.get("title").is_none());
    assert!(obj.get("content").is_none());
    assert!(obj.get("markdown").is_none());
    assert!(obj.get("metadata").is_none());
    assert!(obj.get("schema").is_none());
    assert!(obj.get("parent_document_id").is_none());

    // Required fields should always be present
    assert!(obj.contains_key("uid"));
    assert!(obj.contains_key("url"));
    assert!(obj.contains_key("domain"));
    assert!(obj.contains_key("crawled_at"));
}

// ============================================================================
// Job state tests
// ============================================================================

#[test]
fn test_job_state_lifecycle() {
    use scrapix_core::{JobState, JobStatus};

    let mut job = JobState::new("job-1", "index-1");
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.started_at.is_none());
    assert!(job.completed_at.is_none());

    job.start();
    assert_eq!(job.status, JobStatus::Running);
    assert!(job.started_at.is_some());

    job.complete();
    assert_eq!(job.status, JobStatus::Completed);
    assert!(job.completed_at.is_some());
    assert!(job.duration_seconds().unwrap() >= 0);
}

#[test]
fn test_job_state_failure() {
    use scrapix_core::{JobState, JobStatus};

    let mut job = JobState::new("job-1", "index-1");
    job.start();
    job.fail("Something went wrong");

    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(job.error_message, Some("Something went wrong".to_string()));
    assert!(job.completed_at.is_some());
}

#[test]
fn test_job_state_with_account() {
    use scrapix_core::JobState;

    let job = JobState::with_account("job-1", "index-1", "acct_123");
    assert_eq!(job.account_id, Some("acct_123".to_string()));
}

#[test]
fn test_job_state_serialization_round_trip() {
    use scrapix_core::JobState;

    let mut job = JobState::new("job-1", "index-1");
    job.start();
    job.pages_crawled = 100;
    job.documents_sent = 95;
    job.errors = 5;
    job.bytes_downloaded = 1024 * 1024;

    let json = serde_json::to_string(&job).expect("serialize");
    let d: JobState = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.job_id, "job-1");
    assert_eq!(d.pages_crawled, 100);
    assert_eq!(d.documents_sent, 95);
    assert_eq!(d.errors, 5);
    assert_eq!(d.bytes_downloaded, 1024 * 1024);
}
