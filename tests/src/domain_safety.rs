//! Domain Safety / URL Pattern Tests (P0)
//!
//! Domain explosion is the #1 operational risk in a web crawler.
//! These tests verify that URL extraction properly enforces domain
//! boundaries and pattern filters.

use scrapix_core::{RawPage, UrlPatterns};
use scrapix_crawler::{ExtractorConfig, UrlExtractor, UrlExtractorBuilder};
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
// Allowed domains enforcement
// ============================================================================

#[test]
fn test_allowed_domains_strict_whitelist() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        allowed_domains: vec!["docs.example.com".to_string()],
        ..Default::default()
    });

    let page = make_page(
        "https://docs.example.com/page",
        r#"<html><body>
            <a href="https://docs.example.com/other">Same domain</a>
            <a href="https://example.com/">Parent domain</a>
            <a href="https://blog.example.com/">Sibling subdomain</a>
            <a href="https://evil.com/">External</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(
        urls.len(),
        1,
        "Only exact allowed domain should pass: {:?}",
        urls
    );
    assert!(urls[0].contains("docs.example.com"));
}

#[test]
fn test_allowed_domains_blocks_subdomain_by_default() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        allowed_domains: vec!["example.com".to_string()],
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page">Same</a>
            <a href="https://blog.example.com/post">Subdomain</a>
            <a href="https://docs.example.com/page">Another subdomain</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    // Only example.com should match, not subdomains
    assert_eq!(
        urls.len(),
        1,
        "Allowed domains should be strict: {:?}",
        urls
    );
    assert!(urls[0].starts_with("https://example.com/"));
}

#[test]
fn test_allowed_domains_case_insensitive() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        allowed_domains: vec!["Example.COM".to_string()],
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page">Lowercase</a>
            <a href="https://EXAMPLE.COM/other">Uppercase</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(
        urls.len(),
        2,
        "Domain matching should be case-insensitive: {:?}",
        urls
    );
}

#[test]
fn test_allowed_domains_www_normalization() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        allowed_domains: vec!["example.com".to_string()],
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://www.example.com/page">WWW variant</a>
            <a href="https://example.com/other">Non-WWW</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(
        urls.len(),
        2,
        "www. prefix should be normalized: {:?}",
        urls
    );
}

// ============================================================================
// Subdomain following behavior
// ============================================================================

#[test]
fn test_no_parent_domain_escape_with_subdomains() {
    // Starting from en.wikipedia.org should NOT allow escaping to wikipedia.org
    let extractor = UrlExtractor::new(ExtractorConfig {
        follow_subdomains: true,
        follow_external: false,
        ..Default::default()
    });

    let page = make_page(
        "https://en.wikipedia.org/wiki/Main",
        r#"<html><body>
            <a href="https://en.wikipedia.org/wiki/Article">Same</a>
            <a href="https://wikipedia.org/">Parent domain</a>
            <a href="https://fr.wikipedia.org/wiki/Article">Other subdomain</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);

    // Should only contain en.wikipedia.org URLs
    for url in &urls {
        assert!(
            url.contains("en.wikipedia.org"),
            "Should not escape to parent domain: {url}"
        );
    }
}

#[test]
fn test_subdomain_following_allows_children() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        follow_subdomains: true,
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://docs.example.com/page">Subdomain</a>
            <a href="https://blog.example.com/post">Another subdomain</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 2, "Subdomains should be followed: {:?}", urls);
}

#[test]
fn test_subdomain_following_disabled() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        follow_subdomains: false,
        follow_external: false,
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page">Same domain</a>
            <a href="https://docs.example.com/page">Subdomain</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 1, "Subdomains should be blocked: {:?}", urls);
    assert!(urls[0].contains("example.com/page"));
}

// ============================================================================
// URL pattern filtering
// ============================================================================

#[test]
fn test_include_pattern_filters_urls() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        patterns: Some(UrlPatterns {
            include: vec!["https://example.com/docs/**".to_string()],
            exclude: vec![],
            index_only: vec![],
            allowed_domains: vec![],
        }),
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/docs/intro">Docs</a>
            <a href="https://example.com/docs/api/v2">Deep docs</a>
            <a href="https://example.com/blog/post">Blog (excluded)</a>
            <a href="https://example.com/about">About (excluded)</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 2, "Only /docs/** should match: {:?}", urls);
    for url in &urls {
        assert!(url.contains("/docs/"), "URL should be in docs: {url}");
    }
}

#[test]
fn test_exclude_pattern_takes_priority() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        patterns: Some(UrlPatterns {
            include: vec!["https://example.com/**".to_string()],
            exclude: vec!["**/_internal/**".to_string()],
            index_only: vec![],
            allowed_domains: vec![],
        }),
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/public/page">Public</a>
            <a href="https://example.com/_internal/admin">Internal</a>
            <a href="https://example.com/docs/_internal/debug">Nested internal</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 1, "Exclude should override include: {:?}", urls);
    assert!(urls[0].contains("public/page"));
}

#[test]
fn test_empty_include_patterns_allow_all() {
    let extractor = UrlExtractor::new(ExtractorConfig {
        patterns: Some(UrlPatterns {
            include: vec![], // empty = allow all
            exclude: vec![],
            index_only: vec![],
            allowed_domains: vec![],
        }),
        ..Default::default()
    });

    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page1">Page 1</a>
            <a href="https://example.com/page2">Page 2</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 2, "Empty include should allow all: {:?}", urls);
}

// ============================================================================
// Non-page URL filtering
// ============================================================================

#[test]
fn test_non_page_urls_filtered() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page">HTML page</a>
            <a href="https://example.com/image.png">Image</a>
            <a href="https://example.com/doc.pdf">PDF</a>
            <a href="https://example.com/style.css">CSS</a>
            <a href="https://example.com/app.js">JS</a>
            <a href="https://example.com/font.woff2">Font</a>
            <a href="https://example.com/archive.zip">Archive</a>
            <a href="https://example.com/video.mp4">Video</a>
            <a href="https://example.com/data.json">JSON</a>
        </body></html>"#,
    );

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 1, "Only HTML pages should pass: {:?}", urls);
    assert!(urls[0].contains("/page"));
}

#[test]
fn test_non_page_url_filter_case_insensitive() {
    use scrapix_crawler::is_non_page_url;

    assert!(is_non_page_url("https://example.com/IMAGE.PNG"));
    assert!(is_non_page_url("https://example.com/style.CSS"));
    assert!(is_non_page_url("https://example.com/Video.Mp4"));
}

#[test]
fn test_non_page_url_filter_with_query_params() {
    use scrapix_crawler::is_non_page_url;

    assert!(is_non_page_url("https://example.com/image.png?w=100&h=100"));
    assert!(!is_non_page_url("https://example.com/page?format=json"));
}

// ============================================================================
// Special URL handling
// ============================================================================

#[test]
fn test_javascript_mailto_tel_data_urls_skipped() {
    let extractor = UrlExtractor::with_defaults();
    let html = concat!(
        "<html><body>",
        r#"<a href="javascript:void(0)">JS</a>"#,
        r#"<a href="javascript:alert('xss')">XSS</a>"#,
        r#"<a href="mailto:user@example.com">Email</a>"#,
        r#"<a href="tel:+1234567890">Phone</a>"#,
        r##"<a href="data:text/html,&lt;h1&gt;Hi&lt;/h1&gt;">Data URI</a>"##,
        r##"<a href="#">Hash only</a>"##,
        r#"<a href="">Empty</a>"#,
        r#"<a href="https://example.com/valid">Valid</a>"#,
        "</body></html>",
    );
    let page = make_page("https://example.com", html);

    let urls = extractor.extract_urls(&page);
    assert_eq!(urls.len(), 1);
    assert!(urls[0].contains("valid"));
}

// ============================================================================
// Builder pattern tests
// ============================================================================

#[test]
fn test_url_extractor_builder() {
    let domains = vec!["example.com".to_string()];
    let extractor = UrlExtractorBuilder::new()
        .max_depth(5)
        .follow_external(false)
        .follow_subdomains(true)
        .allowed_domains(domains)
        .build();

    assert_eq!(extractor.config().max_depth, 5);
    assert!(!extractor.config().follow_external);
    assert!(extractor.config().follow_subdomains);
    assert_eq!(extractor.config().allowed_domains.len(), 1);
}

// ============================================================================
// Anchor text extraction
// ============================================================================

#[test]
fn test_anchor_text_extracted() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com",
        r#"<html><body>
            <a href="https://example.com/page">Click here for details</a>
        </body></html>"#,
    );

    let urls = extractor.extract(&page, 0);
    assert_eq!(urls.len(), 1);
    assert_eq!(
        urls[0].anchor_text,
        Some("Click here for details".to_string())
    );
}

#[test]
fn test_parent_url_set_on_extracted_urls() {
    let extractor = UrlExtractor::with_defaults();
    let page = make_page(
        "https://example.com/parent",
        r#"<html><body>
            <a href="https://example.com/child">Child</a>
        </body></html>"#,
    );

    let urls = extractor.extract(&page, 0);
    assert_eq!(urls.len(), 1);
    assert_eq!(
        urls[0].parent_url,
        Some("https://example.com/parent".to_string())
    );
}
