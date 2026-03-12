//! Message Contract Tests (P0)
//!
//! These tests verify that all message types survive serialization round-trips
//! without data loss. This is critical because messages flow between independently
//! deployed services — a single missing `#[serde(default)]` or renamed field
//! breaks the entire pipeline silently.

use scrapix_core::{CrawlUrl, Document, FeaturesConfig, UrlPatterns};
use scrapix_queue::{
    CrawlEvent, CrawlHistoryMessage, DlqMessage, DocumentMessage, LinksMessage, RawPageMessage,
    UrlMessage,
};

// ============================================================================
// UrlMessage round-trip tests
// ============================================================================

#[test]
fn test_url_message_round_trip_all_fields() {
    let url = CrawlUrl::seed("https://example.com/page")
        .with_priority(5)
        .with_parent("https://example.com")
        .with_etag("\"abc123\"")
        .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT");

    let msg = UrlMessage::with_account(url, "job-1", "my-index", "acct_123")
        .with_meilisearch(
            Some("http://meili:7700".to_string()),
            Some("masterKey".to_string()),
        )
        .with_features(Some(FeaturesConfig::from_cli_args(
            true, true, true, true, true, true, None,
        )))
        .with_limits(Some(3), Some(100));

    let json = serde_json::to_string(&msg).expect("serialize UrlMessage");
    let deserialized: UrlMessage = serde_json::from_str(&json).expect("deserialize UrlMessage");

    assert_eq!(deserialized.url.url, "https://example.com/page");
    assert_eq!(deserialized.url.priority, 5);
    assert_eq!(
        deserialized.url.parent_url,
        Some("https://example.com".to_string())
    );
    assert_eq!(deserialized.url.etag, Some("\"abc123\"".to_string()));
    assert_eq!(
        deserialized.url.last_modified,
        Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
    );
    assert_eq!(deserialized.job_id, "job-1");
    assert_eq!(deserialized.index_uid, "my-index");
    assert_eq!(deserialized.account_id, Some("acct_123".to_string()));
    assert_eq!(
        deserialized.meilisearch_url,
        Some("http://meili:7700".to_string())
    );
    assert_eq!(
        deserialized.meilisearch_api_key,
        Some("masterKey".to_string())
    );
    assert!(deserialized.features.is_some());
    assert_eq!(deserialized.max_depth, Some(3));
    assert_eq!(deserialized.max_pages, Some(100));
    assert!(!deserialized.message_id.is_empty());
    assert!(deserialized.created_at > 0);
}

#[test]
fn test_url_message_round_trip_minimal() {
    let url = CrawlUrl::seed("https://example.com");
    let msg = UrlMessage::new(url, "job-1", "index-1");

    let json = serde_json::to_string(&msg).expect("serialize");
    let deserialized: UrlMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.url.url, "https://example.com");
    assert_eq!(deserialized.job_id, "job-1");
    assert_eq!(deserialized.index_uid, "index-1");
    assert!(deserialized.account_id.is_none());
    assert!(deserialized.meilisearch_url.is_none());
    assert!(deserialized.features.is_none());
    assert!(deserialized.max_depth.is_none());
    assert!(deserialized.max_pages.is_none());
    assert!(deserialized.url_patterns.is_none());
}

#[test]
fn test_url_message_with_patterns_round_trip() {
    let url = CrawlUrl::seed("https://example.com");
    let patterns = UrlPatterns {
        include: vec!["https://example.com/docs/**".to_string()],
        exclude: vec!["**/_internal/**".to_string()],
        index_only: vec!["https://example.com/docs/public/**".to_string()],
        allowed_domains: vec!["example.com".to_string(), "docs.example.com".to_string()],
    };

    let msg = UrlMessage::with_patterns(url, "job-1", "index-1", patterns);

    let json = serde_json::to_string(&msg).expect("serialize");
    let deserialized: UrlMessage = serde_json::from_str(&json).expect("deserialize");

    let patterns = deserialized.url_patterns.expect("patterns should exist");
    assert_eq!(patterns.include.len(), 1);
    assert_eq!(patterns.exclude.len(), 1);
    assert_eq!(patterns.index_only.len(), 1);
    assert_eq!(patterns.allowed_domains.len(), 2);
}

/// Backward compatibility: messages from older services without new fields
/// must still deserialize correctly with defaults.
#[test]
fn test_url_message_deserialize_without_optional_fields() {
    // Simulate a message from an older service that doesn't have features/limits
    let json = r#"{
        "url": {"url": "https://example.com", "depth": 0, "priority": 0, "parent_url": null, "anchor_text": null, "discovered_at": "2024-01-01T00:00:00Z", "retry_count": 0, "requires_js": false},
        "job_id": "job-old",
        "index_uid": "index-old",
        "message_id": "msg-1",
        "created_at": 1704067200000
    }"#;

    let msg: UrlMessage = serde_json::from_str(json).expect("should deserialize old format");
    assert_eq!(msg.job_id, "job-old");
    assert!(msg.account_id.is_none());
    assert!(msg.features.is_none());
    assert!(msg.max_depth.is_none());
    assert!(msg.max_pages.is_none());
    assert!(msg.meilisearch_url.is_none());
    assert!(msg.url_patterns.is_none());
}

// ============================================================================
// RawPageMessage round-trip tests
// ============================================================================

#[test]
fn test_raw_page_message_round_trip_all_fields() {
    let msg = RawPageMessage {
        url: "https://example.com/page".to_string(),
        final_url: "https://example.com/page-redirected".to_string(),
        status: 200,
        html: "<html><body>Hello</body></html>".to_string(),
        content_type: Some("text/html; charset=utf-8".to_string()),
        content_length: 12345,
        js_rendered: true,
        fetched_at: 1704067200000,
        fetch_duration_ms: 250,
        job_id: "job-1".to_string(),
        index_uid: "my-index".to_string(),
        account_id: Some("acct_123".to_string()),
        source: None,
        message_id: "msg-1".to_string(),
        etag: Some("\"etag-value\"".to_string()),
        last_modified: Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string()),
        meilisearch_url: Some("http://meili:7700".to_string()),
        meilisearch_api_key: Some("key123".to_string()),
        features: Some(FeaturesConfig::default()),
    };

    let json = serde_json::to_string(&msg).expect("serialize");
    let d: RawPageMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.url, "https://example.com/page");
    assert_eq!(d.final_url, "https://example.com/page-redirected");
    assert_eq!(d.status, 200);
    assert_eq!(d.content_length, 12345);
    assert!(d.js_rendered);
    assert_eq!(d.account_id, Some("acct_123".to_string()));
    assert_eq!(d.etag, Some("\"etag-value\"".to_string()));
    assert_eq!(
        d.last_modified,
        Some("Wed, 21 Oct 2023 07:28:00 GMT".to_string())
    );
    assert_eq!(d.meilisearch_url, Some("http://meili:7700".to_string()));
    assert_eq!(d.meilisearch_api_key, Some("key123".to_string()));
    assert!(d.features.is_some());
}

#[test]
fn test_raw_page_message_deserialize_without_optional_fields() {
    let json = r#"{
        "url": "https://example.com",
        "final_url": "https://example.com",
        "status": 200,
        "html": "<html></html>",
        "content_type": null,
        "js_rendered": false,
        "fetched_at": 1704067200000,
        "fetch_duration_ms": 100,
        "job_id": "job-1",
        "index_uid": "index-1",
        "message_id": "msg-1"
    }"#;

    let msg: RawPageMessage = serde_json::from_str(json).expect("should deserialize");
    assert_eq!(msg.content_length, 0); // default
    assert!(msg.account_id.is_none());
    assert!(msg.etag.is_none());
    assert!(msg.last_modified.is_none());
    assert!(msg.meilisearch_url.is_none());
    assert!(msg.features.is_none());
}

#[test]
fn test_raw_page_message_with_large_html() {
    let large_html = "x".repeat(10_000_000); // 10MB
    let msg = RawPageMessage {
        url: "https://example.com".to_string(),
        final_url: "https://example.com".to_string(),
        status: 200,
        html: large_html.clone(),
        content_type: Some("text/html".to_string()),
        content_length: large_html.len() as u64,
        js_rendered: false,
        fetched_at: 1704067200000,
        fetch_duration_ms: 5000,
        job_id: "job-1".to_string(),
        index_uid: "index-1".to_string(),
        account_id: None,
        source: None,
        message_id: "msg-1".to_string(),
        etag: None,
        last_modified: None,
        meilisearch_url: None,
        meilisearch_api_key: None,
        features: None,
    };

    let json = serde_json::to_string(&msg).expect("serialize large HTML");
    let d: RawPageMessage = serde_json::from_str(&json).expect("deserialize large HTML");
    assert_eq!(d.html.len(), 10_000_000);
    assert_eq!(d.content_length, 10_000_000);
}

// ============================================================================
// CrawlEvent round-trip tests (all variants)
// ============================================================================

#[test]
fn test_crawl_event_job_started_round_trip() {
    let event = CrawlEvent::job_started_with_account(
        "job-1",
        "index-1",
        "acct_123",
        vec![
            "https://example.com".to_string(),
            "https://example.com/docs".to_string(),
        ],
    );

    let json = serde_json::to_string(&event).expect("serialize");
    let d: CrawlEvent = serde_json::from_str(&json).expect("deserialize");

    match d {
        CrawlEvent::JobStarted {
            job_id,
            index_uid,
            account_id,
            start_urls,
            timestamp,
        } => {
            assert_eq!(job_id, "job-1");
            assert_eq!(index_uid, "index-1");
            assert_eq!(account_id, Some("acct_123".to_string()));
            assert_eq!(start_urls.len(), 2);
            assert!(timestamp > 0);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_crawl_event_job_completed_round_trip() {
    let json = r#"{
        "type": "job_completed",
        "job_id": "job-1",
        "account_id": "acct_123",
        "pages_crawled": 500,
        "documents_indexed": 480,
        "errors": 20,
        "bytes_downloaded": 52428800,
        "duration_secs": 120,
        "timestamp": 1704067200000
    }"#;

    let event: CrawlEvent = serde_json::from_str(json).expect("deserialize");
    match event {
        CrawlEvent::JobCompleted {
            pages_crawled,
            documents_indexed,
            errors,
            bytes_downloaded,
            duration_secs,
            ..
        } => {
            assert_eq!(pages_crawled, 500);
            assert_eq!(documents_indexed, 480);
            assert_eq!(errors, 20);
            assert_eq!(bytes_downloaded, 52428800);
            assert_eq!(duration_secs, 120);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_crawl_event_page_crawled_with_billing_round_trip() {
    let event = CrawlEvent::page_crawled_with_billing(
        "job-1",
        Some("acct_123".to_string()),
        "https://example.com/page",
        200,
        54321,
        150,
    );

    let json = serde_json::to_string(&event).expect("serialize");
    let d: CrawlEvent = serde_json::from_str(&json).expect("deserialize");

    match d {
        CrawlEvent::PageCrawled {
            content_length,
            account_id,
            status,
            ..
        } => {
            assert_eq!(content_length, 54321);
            assert_eq!(account_id, Some("acct_123".to_string()));
            assert_eq!(status, 200);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_crawl_event_page_failed_round_trip() {
    let event = CrawlEvent::page_failed("job-1", "https://example.com/fail", "Connection reset", 3);

    let json = serde_json::to_string(&event).expect("serialize");
    let d: CrawlEvent = serde_json::from_str(&json).expect("deserialize");

    match d {
        CrawlEvent::PageFailed {
            url,
            error,
            retry_count,
            ..
        } => {
            assert_eq!(url, "https://example.com/fail");
            assert_eq!(error, "Connection reset");
            assert_eq!(retry_count, 3);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_crawl_event_all_variants_deserialize() {
    let variants = vec![
        r#"{"type":"job_started","job_id":"j","index_uid":"i","start_urls":[],"timestamp":0}"#,
        r#"{"type":"job_completed","job_id":"j","pages_crawled":0,"documents_indexed":0,"errors":0,"duration_secs":0,"timestamp":0}"#,
        r#"{"type":"job_failed","job_id":"j","error":"boom","timestamp":0}"#,
        r#"{"type":"page_crawled","job_id":"j","url":"u","status":200,"duration_ms":0,"timestamp":0}"#,
        r#"{"type":"page_failed","job_id":"j","url":"u","error":"e","retry_count":0,"timestamp":0}"#,
        r#"{"type":"document_indexed","job_id":"j","url":"u","document_id":"d","timestamp":0}"#,
        r#"{"type":"urls_discovered","job_id":"j","source_url":"u","count":5,"timestamp":0}"#,
        r#"{"type":"rate_limited","job_id":"j","domain":"d","wait_ms":100,"timestamp":0}"#,
        r#"{"type":"page_skipped","job_id":"j","url":"u","reason":"dup","timestamp":0}"#,
    ];

    for json in variants {
        let event: CrawlEvent = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("Failed to deserialize: {json}\nError: {e}"));
        // Re-serialize to verify round-trip
        let re_json = serde_json::to_string(&event).expect("re-serialize");
        let _: CrawlEvent = serde_json::from_str(&re_json).expect("round-trip deserialize");
    }
}

/// Backward compat: older events without account_id should deserialize with None
#[test]
fn test_crawl_event_without_account_id() {
    let json = r#"{"type":"page_crawled","job_id":"j","url":"u","status":200,"duration_ms":50,"timestamp":0}"#;

    let event: CrawlEvent = serde_json::from_str(json).expect("deserialize");
    match event {
        CrawlEvent::PageCrawled {
            account_id,
            content_length,
            ..
        } => {
            assert!(account_id.is_none());
            assert_eq!(content_length, 0); // default
        }
        _ => panic!("Wrong variant"),
    }
}

// ============================================================================
// DocumentMessage round-trip
// ============================================================================

#[test]
fn test_document_message_round_trip_full_document() {
    let mut doc = Document::new("https://example.com/page", "example.com");
    doc.title = Some("Test Page".to_string());
    doc.content = Some("Full content here".to_string());
    doc.markdown = Some("# Test Page\n\nFull content here".to_string());
    doc.language = Some("en".to_string());
    doc.h1 = Some("Test Page".to_string());
    doc.h2 = Some("Section".to_string());
    doc.anchor = Some("section-1".to_string());

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("description".to_string(), "A test page".to_string());
    metadata.insert("author".to_string(), "Test".to_string());
    doc.metadata = Some(metadata);

    doc.schema = Some(serde_json::json!({
        "@type": "Article",
        "name": "Test"
    }));

    let msg = DocumentMessage::new(doc, "job-1", "index-1");

    let json = serde_json::to_string(&msg).expect("serialize");
    let d: DocumentMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.document.url, "https://example.com/page");
    assert_eq!(d.document.title, Some("Test Page".to_string()));
    assert_eq!(d.document.language, Some("en".to_string()));
    assert_eq!(d.document.h1, Some("Test Page".to_string()));
    assert_eq!(d.document.anchor, Some("section-1".to_string()));
    assert!(d.document.metadata.is_some());
    assert!(d.document.schema.is_some());
    assert!(d.document.markdown.is_some());
}

#[test]
fn test_document_message_block_document_round_trip() {
    let parent = Document::new("https://example.com/page", "example.com");
    let mut block = Document::new_block(&parent, 2);
    block.content = Some("Block content".to_string());
    block.h1 = Some("Main Title".to_string());
    block.h2 = Some("Block Section".to_string());
    block.anchor = Some("block-2".to_string());

    let msg = DocumentMessage::new(block, "job-1", "index-1");
    let json = serde_json::to_string(&msg).expect("serialize");
    let d: DocumentMessage = serde_json::from_str(&json).expect("deserialize");

    assert!(d.document.is_block());
    assert_eq!(d.document.parent_document_id, Some(parent.uid));
    assert_eq!(d.document.page_block, Some(2));
}

// ============================================================================
// DlqMessage round-trip
// ============================================================================

#[test]
fn test_dlq_message_round_trip() {
    let msg = DlqMessage::new(
        r#"{"url":"https://example.com"}"#,
        "scrapix.urls.frontier",
        "Connection refused",
    )
    .with_job_id("job-1")
    .increment_retry()
    .increment_retry();

    let json = serde_json::to_string(&msg).expect("serialize");
    let d: DlqMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.retry_count, 3); // 1 initial + 2 increments
    assert_eq!(d.original_topic, "scrapix.urls.frontier");
    assert_eq!(d.job_id, Some("job-1".to_string()));
    assert!(d.original_message.contains("example.com"));
}

// ============================================================================
// LinksMessage and CrawlHistoryMessage round-trips
// ============================================================================

#[test]
fn test_links_message_round_trip() {
    let msg = LinksMessage::new(
        "https://example.com/source",
        vec![
            "https://example.com/target1".to_string(),
            "https://example.com/target2".to_string(),
        ],
        "job-1",
    );

    let json = serde_json::to_string(&msg).expect("serialize");
    let d: LinksMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.source_url, msg.source_url);
    assert_eq!(d.target_urls.len(), 2);
}

#[test]
fn test_crawl_history_message_round_trip_all_fields() {
    let msg = CrawlHistoryMessage::new("https://example.com/page", 200, "job-1")
        .with_etag("\"etag-val\"")
        .with_last_modified("Wed, 21 Oct 2023 07:28:00 GMT")
        .with_content_hash("sha256:abcdef")
        .with_content_length(99999)
        .with_content_changed(false);

    let json = serde_json::to_string(&msg).expect("serialize");
    let d: CrawlHistoryMessage = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.url, "https://example.com/page");
    assert_eq!(d.status, 200);
    assert_eq!(d.etag, Some("\"etag-val\"".to_string()));
    assert_eq!(d.content_hash, Some("sha256:abcdef".to_string()));
    assert_eq!(d.content_length, Some(99999));
    assert!(!d.content_changed);
}

// ============================================================================
// Partition key tests
// ============================================================================

#[test]
fn test_partition_key_extracts_domain() {
    let msg = UrlMessage::new(
        CrawlUrl::seed("https://docs.example.com/api/v2"),
        "job-1",
        "idx",
    );
    assert_eq!(msg.partition_key(), "docs.example.com");
}

#[test]
fn test_partition_key_stable_across_url_variants() {
    let urls = vec![
        "https://example.com/page1",
        "https://example.com/page2?q=1",
        "https://example.com/deep/nested/page",
        "https://example.com",
    ];

    let keys: Vec<String> = urls
        .into_iter()
        .map(|u| UrlMessage::new(CrawlUrl::seed(u), "j", "i").partition_key())
        .collect();

    // All URLs from the same domain must have the same partition key
    assert!(keys.iter().all(|k| k == "example.com"));
}

#[test]
fn test_partition_key_for_malformed_url_falls_back() {
    let url = CrawlUrl::seed("not-a-valid-url");
    let msg = UrlMessage::new(url, "job-fallback", "idx");

    // Should fall back to job_id when URL can't be parsed
    assert_eq!(msg.partition_key(), "job-fallback");
}
