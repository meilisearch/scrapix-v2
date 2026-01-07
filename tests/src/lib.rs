//! Integration test helpers and fixtures for Scrapix
//!
//! This module provides common utilities for integration testing:
//! - HTML fixtures for parser testing
//! - Mock HTTP server setup
//! - Test document builders

use chrono::Utc;
use scrapix_core::{CrawlUrl, Document, RawPage};
use std::collections::HashMap;

/// Sample HTML pages for testing
pub mod fixtures {
    /// Simple article page
    pub const SIMPLE_ARTICLE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Test Article Title</title>
    <meta name="description" content="This is a test article description for integration testing.">
    <meta name="author" content="Test Author">
    <meta property="og:title" content="Test Article OG Title">
    <meta property="og:description" content="OG description for testing">
    <meta property="og:type" content="article">
</head>
<body>
    <header>
        <nav>
            <a href="/">Home</a>
            <a href="/about">About</a>
            <a href="/contact">Contact</a>
        </nav>
    </header>
    <main>
        <article>
            <h1>Test Article Title</h1>
            <p class="author">By Test Author</p>
            <p class="date">Published on January 1, 2024</p>
            <p>This is the first paragraph of the test article. It contains some meaningful content that should be extracted by the parser.</p>
            <h2>Section One</h2>
            <p>This is the content of section one. It discusses important topics related to web crawling and content extraction.</p>
            <h2>Section Two</h2>
            <p>Section two covers additional information about the testing process and how integration tests work.</p>
            <ul>
                <li>First list item</li>
                <li>Second list item</li>
                <li>Third list item</li>
            </ul>
            <h3>Subsection</h3>
            <p>A subsection with more detailed content about specific implementation details.</p>
        </article>
    </main>
    <aside>
        <h3>Related Articles</h3>
        <a href="/article/1">Related Article 1</a>
        <a href="/article/2">Related Article 2</a>
    </aside>
    <footer>
        <p>Copyright 2024 Test Site</p>
        <a href="/privacy">Privacy Policy</a>
    </footer>
</body>
</html>
"#;

    /// Page with JSON-LD schema
    pub const PAGE_WITH_SCHEMA: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <title>Product Page</title>
    <script type="application/ld+json">
    {
        "@context": "https://schema.org",
        "@type": "Product",
        "name": "Test Product",
        "description": "A great product for testing",
        "brand": {
            "@type": "Brand",
            "name": "Test Brand"
        },
        "offers": {
            "@type": "Offer",
            "price": "99.99",
            "priceCurrency": "USD",
            "availability": "https://schema.org/InStock"
        }
    }
    </script>
</head>
<body>
    <main>
        <h1>Test Product</h1>
        <p>A great product for testing purposes.</p>
        <p class="price">$99.99</p>
    </main>
</body>
</html>
"#;

    /// Page with multiple links for URL extraction testing
    pub const PAGE_WITH_LINKS: &str = r#"
<!DOCTYPE html>
<html>
<head><title>Links Page</title></head>
<body>
    <a href="/page1">Internal Page 1</a>
    <a href="/page2">Internal Page 2</a>
    <a href="https://example.com/page3">Same Domain Page 3</a>
    <a href="https://other-domain.com/external">External Link</a>
    <a href="https://another.com/page">Another External</a>
    <a href="/page4?query=test">Page with Query</a>
    <a href="/page5#section">Page with Fragment</a>
    <a href="mailto:test@example.com">Email Link</a>
    <a href="javascript:void(0)">JavaScript Link</a>
    <a href="">Empty Link</a>
</body>
</html>
"#;

    /// Minimal page
    pub const MINIMAL_PAGE: &str = r#"
<!DOCTYPE html>
<html>
<head><title>Minimal</title></head>
<body><p>Minimal content.</p></body>
</html>
"#;

    /// Page with no content (should be skipped)
    pub const EMPTY_CONTENT_PAGE: &str = r#"
<!DOCTYPE html>
<html>
<head><title>Empty</title></head>
<body>
    <nav><a href="/">Home</a></nav>
    <footer>Copyright</footer>
</body>
</html>
"#;

    /// Non-English page for language detection
    pub const FRENCH_PAGE: &str = r#"
<!DOCTYPE html>
<html lang="fr">
<head>
    <title>Article en Français</title>
    <meta name="description" content="Un article de test en français">
</head>
<body>
    <article>
        <h1>Bienvenue sur notre site</h1>
        <p>Ceci est un article en français pour tester la détection de langue.
        Le contenu est suffisamment long pour permettre une détection précise.</p>
        <p>La France est un pays situé en Europe occidentale. Paris est sa capitale
        et sa plus grande ville. Le français est la langue officielle du pays.</p>
    </article>
</body>
</html>
"#;
}

/// Create a RawPage for testing
pub fn create_raw_page(url: &str, html: &str) -> RawPage {
    RawPage {
        url: url.to_string(),
        final_url: url.to_string(),
        status: 200,
        headers: HashMap::new(),
        html: html.to_string(),
        content_type: Some("text/html; charset=utf-8".to_string()),
        js_rendered: false,
        fetched_at: Utc::now(),
        fetch_duration_ms: 100,
    }
}

/// Create a CrawlUrl for testing
pub fn create_crawl_url(url: &str, depth: u32) -> CrawlUrl {
    CrawlUrl::new(url, depth)
}

/// Create a test document
pub fn create_test_document(url: &str) -> Document {
    let domain = url::Url::parse(url)
        .map(|u| u.host_str().unwrap_or("unknown").to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let mut doc = Document::new(url, domain);
    doc.title = Some("Test Document".to_string());
    doc.content = Some("Test content for the document.".to_string());
    doc
}

/// Initialize tracing for tests (call once per test module)
pub fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

/// URL generation helpers for testing
pub mod urls {
    pub fn generate_urls(domain: &str, count: usize) -> Vec<String> {
        (0..count)
            .map(|i| format!("https://{}/page/{}", domain, i))
            .collect()
    }

    pub fn generate_urls_multi_domain(domains: &[&str], per_domain: usize) -> Vec<String> {
        domains
            .iter()
            .flat_map(|domain| generate_urls(domain, per_domain))
            .collect()
    }
}
