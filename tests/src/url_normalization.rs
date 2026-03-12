//! URL Normalization Consistency Tests (P1)
//!
//! URLs are normalized in multiple places: dedup bloom filter, link extractor,
//! partition key computation. If these normalize differently, URLs get crawled
//! twice or dedup misses them.

use scrapix_core::{CrawlUrl, RawPage};
use scrapix_crawler::UrlExtractor;
use scrapix_frontier::UrlDedup;
use scrapix_queue::UrlMessage;
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
// Dedup normalization
// ============================================================================

#[test]
fn test_dedup_trailing_slash_consistency() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    dedup.mark_seen("https://example.com/page/");
    assert!(
        dedup.is_seen("https://example.com/page"),
        "URL without trailing slash should match URL with trailing slash"
    );

    dedup.mark_seen("https://example.com/other");
    assert!(
        dedup.is_seen("https://example.com/other/"),
        "URL with trailing slash should match URL without trailing slash"
    );
}

#[test]
fn test_dedup_case_insensitive_host() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    dedup.mark_seen("https://EXAMPLE.COM/page");
    assert!(dedup.is_seen("https://example.com/page"));
    assert!(dedup.is_seen("https://Example.Com/page"));
}

#[test]
fn test_dedup_fragment_stripped() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    dedup.mark_seen("https://example.com/page#section-1");
    assert!(dedup.is_seen("https://example.com/page"));
    assert!(dedup.is_seen("https://example.com/page#other-section"));
}

#[test]
fn test_dedup_query_params_stripped() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    dedup.mark_seen("https://example.com/page?utm_source=google&ref=123");
    assert!(dedup.is_seen("https://example.com/page"));
    assert!(dedup.is_seen("https://example.com/page?other=param"));
}

#[test]
fn test_dedup_default_port_handling() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);

    // https default port is 443
    dedup.mark_seen("https://example.com:443/page");
    // After normalization by url::Url, :443 is stripped for https
    assert!(
        dedup.is_seen("https://example.com/page"),
        "Default port should be normalized away"
    );
}

// ============================================================================
// Dedup ↔ Extractor consistency
// ============================================================================

#[test]
fn test_dedup_and_extractor_agree_on_trailing_slash() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);
    let extractor = UrlExtractor::with_defaults();

    // Extractor normalizes links from HTML
    let page = make_page(
        "https://example.com",
        r#"<html><body><a href="https://example.com/docs/">Docs</a></body></html>"#,
    );

    let extracted = extractor.extract_urls(&page);
    assert_eq!(extracted.len(), 1);

    // Mark the extracted URL as seen
    dedup.mark_seen(&extracted[0]);

    // The same URL without trailing slash should be seen
    assert!(
        dedup.is_seen("https://example.com/docs"),
        "Dedup should recognize the URL without trailing slash after extractor normalized it"
    );
}

#[test]
fn test_dedup_and_extractor_agree_on_fragment_removal() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);
    let extractor = UrlExtractor::with_defaults();

    let page = make_page(
        "https://example.com",
        r#"<html><body><a href="https://example.com/page#section">Link</a></body></html>"#,
    );

    let extracted = extractor.extract_urls(&page);

    // Extractor should strip fragments
    for url in &extracted {
        assert!(
            !url.contains('#'),
            "Extractor should strip fragments, got: {url}"
        );
    }

    // Mark extracted URL in dedup
    if !extracted.is_empty() {
        dedup.mark_seen(&extracted[0]);
        assert!(dedup.is_seen("https://example.com/page"));
    }
}

#[test]
fn test_dedup_and_extractor_agree_on_query_param_removal() {
    let dedup = UrlDedup::for_capacity(1000, 0.01);
    let extractor = UrlExtractor::with_defaults();

    let page = make_page(
        "https://example.com",
        r#"<html><body><a href="https://example.com/search?q=test&page=2">Search</a></body></html>"#,
    );

    let extracted = extractor.extract_urls(&page);

    // Both should agree on what the normalized URL is
    if !extracted.is_empty() {
        dedup.mark_seen(&extracted[0]);

        // All these variants should be seen
        assert!(dedup.is_seen("https://example.com/search"));
        assert!(dedup.is_seen("https://example.com/search?q=other"));
    }
}

// ============================================================================
// Partition key consistency
// ============================================================================

#[test]
fn test_partition_key_consistent_for_same_domain() {
    let urls = vec![
        "https://example.com/",
        "https://example.com/page",
        "https://example.com/deep/nested/path",
        "https://example.com/page?q=1",
        "https://example.com/page#section",
    ];

    let keys: Vec<String> = urls
        .into_iter()
        .map(|u| UrlMessage::new(CrawlUrl::seed(u), "j", "i").partition_key())
        .collect();

    let first = &keys[0];
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(
            key, first,
            "Partition key mismatch at index {i}: {key} != {first}"
        );
    }
}

#[test]
fn test_partition_key_different_for_different_domains() {
    let key1 =
        UrlMessage::new(CrawlUrl::seed("https://example.com/page"), "j", "i").partition_key();
    let key2 = UrlMessage::new(CrawlUrl::seed("https://other.com/page"), "j", "i").partition_key();

    assert_ne!(key1, key2);
}

#[test]
fn test_partition_key_handles_ip_addresses() {
    let msg = UrlMessage::new(CrawlUrl::seed("http://192.168.1.1:8080/api"), "j", "i");
    let key = msg.partition_key();
    assert_eq!(key, "192.168.1.1");
}

#[test]
fn test_partition_key_handles_localhost() {
    let msg = UrlMessage::new(CrawlUrl::seed("http://localhost:3000/page"), "j", "i");
    let key = msg.partition_key();
    assert_eq!(key, "localhost");
}

// ============================================================================
// Extractor normalization edge cases
// ============================================================================

#[test]
fn test_extractor_skips_non_http_schemes() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="ftp://files.example.com/doc">FTP</a>
            <a href="file:///tmp/local">Local</a>
            <a href="https://example.com/valid">Valid</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 1);
    assert!(urls[0].contains("example.com/valid"));
}

#[test]
fn test_extractor_handles_empty_href() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="">Empty</a>
            <a href="   ">Whitespace</a>
            <a href="https://example.com/valid">Valid</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 1);
}

#[test]
fn test_extractor_deduplicates_within_page() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page">Link 1</a>
            <a href="https://example.com/page">Link 2</a>
            <a href="https://example.com/page?ref=nav">Link 3</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    // All three should normalize to the same URL (query params stripped)
    assert_eq!(urls.len(), 1, "Should deduplicate within page: {:?}", urls);
}

#[test]
fn test_extractor_depth_increments() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com",
        r#"<html><body><a href="https://example.com/child">Child</a></body></html>"#,
    );

    let urls = extractor.extract(&page, 3);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0].depth, 4, "Depth should be parent_depth + 1");
}

#[test]
fn test_extractor_respects_max_depth() {
    let extractor = UrlExtractor::new(scrapix_crawler::ExtractorConfig {
        max_depth: 3,
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body><a href="https://example.com/deep">Deep</a></body></html>"#,
    );

    // At depth 3 (== max_depth), should return nothing
    let urls = extractor.extract(&page, 3);
    assert!(urls.is_empty(), "Should not extract at max depth");

    // At depth 2, should extract (child will be depth 3)
    let urls = extractor.extract(&page, 2);
    assert_eq!(urls.len(), 1);
}
