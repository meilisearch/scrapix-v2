//! Performance benchmarks for Scrapix Parser
//!
//! Run with: cargo bench -p scrapix-parser
//!
//! These benchmarks measure:
//! - HTML parsing throughput
//! - Markdown conversion speed
//! - Content extraction performance
//! - Language detection speed

use std::collections::HashMap;

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use scrapix_core::RawPage;
use scrapix_parser::{
    extract_content, html_to_markdown, HtmlParser, HtmlParserBuilder,
    detect_language,
};

// =============================================================================
// TEST HTML CONTENT
// =============================================================================

/// Simple HTML page
const SIMPLE_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Simple Page</title>
    <meta name="description" content="A simple test page">
</head>
<body>
    <h1>Hello World</h1>
    <p>This is a simple paragraph with some text content.</p>
    <a href="/page1">Link 1</a>
    <a href="/page2">Link 2</a>
</body>
</html>
"#;

/// Medium complexity HTML page
const MEDIUM_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <title>Documentation Page</title>
    <meta name="description" content="A documentation page with code examples">
    <meta property="og:title" content="Documentation">
    <meta property="og:type" content="article">
</head>
<body>
    <header>
        <nav>
            <a href="/">Home</a>
            <a href="/docs">Docs</a>
            <a href="/api">API</a>
        </nav>
    </header>
    <main>
        <article>
            <h1>Getting Started</h1>
            <p>Welcome to our documentation. This guide will help you get started with the framework.</p>

            <h2>Installation</h2>
            <p>You can install the package using npm:</p>
            <pre><code>npm install my-package</code></pre>

            <h2>Basic Usage</h2>
            <p>Here's a simple example:</p>
            <pre><code>
const myPackage = require('my-package');
myPackage.init({ debug: true });
            </code></pre>

            <h3>Configuration Options</h3>
            <table>
                <tr><th>Option</th><th>Default</th><th>Description</th></tr>
                <tr><td>debug</td><td>false</td><td>Enable debug mode</td></tr>
                <tr><td>timeout</td><td>30000</td><td>Request timeout in ms</td></tr>
            </table>

            <h2>Advanced Topics</h2>
            <ul>
                <li><a href="/docs/plugins">Plugins</a></li>
                <li><a href="/docs/api">API Reference</a></li>
                <li><a href="/docs/faq">FAQ</a></li>
            </ul>
        </article>
    </main>
    <footer>
        <p>Copyright 2024</p>
    </footer>
</body>
</html>
"#;

/// Generate a large HTML document with many elements
fn generate_large_html(paragraphs: usize, links: usize) -> String {
    let mut html = String::from(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <title>Large Document</title>
    <meta name="description" content="A large test document">
    <meta property="og:title" content="Large Document">
    <meta property="og:description" content="Testing with large content">
</head>
<body>
    <header><nav><a href="/">Home</a></nav></header>
    <main>
        <article>
            <h1>Large Document Title</h1>
"#,
    );

    for i in 0..paragraphs {
        html.push_str(&format!(
            r#"
            <h2>Section {}</h2>
            <p>This is paragraph {} with some content that makes it realistic.
               It contains multiple sentences and some <strong>bold</strong> and
               <em>italic</em> text. Lorem ipsum dolor sit amet, consectetur
               adipiscing elit. Sed do eiusmod tempor incididunt ut labore et
               dolore magna aliqua.</p>
"#,
            i, i
        ));

        if i % 5 == 0 {
            for j in 0..(links / (paragraphs / 5 + 1)) {
                html.push_str(&format!(
                    r#"            <a href="/page{}_{}">Link {} in section {}</a>
"#,
                    i, j, j, i
                ));
            }
        }
    }

    html.push_str(
        r#"
        </article>
    </main>
    <footer><p>Copyright 2024</p></footer>
</body>
</html>
"#,
    );

    html
}

/// Create a RawPage from HTML content
fn create_raw_page(html: &str) -> RawPage {
    RawPage {
        url: "https://example.com/test".to_string(),
        final_url: "https://example.com/test".to_string(),
        status: 200,
        headers: HashMap::new(),
        html: html.to_string(),
        content_type: Some("text/html".to_string()),
        js_rendered: false,
        fetched_at: Utc::now(),
        fetch_duration_ms: 100,
    }
}

// =============================================================================
// HTML PARSING BENCHMARKS
// =============================================================================

fn bench_html_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("html_parsing");

    // Simple HTML
    let simple_page = create_raw_page(SIMPLE_HTML);
    group.throughput(Throughput::Bytes(SIMPLE_HTML.len() as u64));
    group.bench_function("simple_html", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(false)
            .detect_language(false)
            .build();
        b.iter(|| {
            black_box(parser.parse(&simple_page).unwrap());
        });
    });

    // Medium HTML
    let medium_page = create_raw_page(MEDIUM_HTML);
    group.throughput(Throughput::Bytes(MEDIUM_HTML.len() as u64));
    group.bench_function("medium_html", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(false)
            .detect_language(false)
            .build();
        b.iter(|| {
            black_box(parser.parse(&medium_page).unwrap());
        });
    });

    // Large HTML (100 paragraphs, 500 links)
    let large_html = generate_large_html(100, 500);
    let large_page = create_raw_page(&large_html);
    group.throughput(Throughput::Bytes(large_html.len() as u64));
    group.bench_function("large_html_100p", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(false)
            .detect_language(false)
            .build();
        b.iter(|| {
            black_box(parser.parse(&large_page).unwrap());
        });
    });

    // Very large HTML (500 paragraphs, 2000 links)
    let very_large_html = generate_large_html(500, 2000);
    let very_large_page = create_raw_page(&very_large_html);
    group.throughput(Throughput::Bytes(very_large_html.len() as u64));
    group.bench_function("very_large_html_500p", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(false)
            .detect_language(false)
            .build();
        b.iter(|| {
            black_box(parser.parse(&very_large_page).unwrap());
        });
    });

    group.finish();
}

fn bench_full_parsing_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_parsing_pipeline");

    let medium_page = create_raw_page(MEDIUM_HTML);

    // Parser only
    group.bench_function("parse_only", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(false)
            .detect_language(false)
            .build();
        b.iter(|| {
            black_box(parser.parse(&medium_page).unwrap());
        });
    });

    // Parser + Markdown
    group.bench_function("parse_and_markdown", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(true)
            .detect_language(false)
            .build();
        b.iter(|| {
            black_box(parser.parse(&medium_page).unwrap());
        });
    });

    // Parser + Markdown + Language
    group.bench_function("parse_markdown_language", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(true)
            .detect_language(true)
            .build();
        b.iter(|| {
            black_box(parser.parse(&medium_page).unwrap());
        });
    });

    group.finish();
}

// =============================================================================
// MARKDOWN CONVERSION BENCHMARKS
// =============================================================================

fn bench_markdown_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_conversion");

    // Simple HTML
    group.throughput(Throughput::Bytes(SIMPLE_HTML.len() as u64));
    group.bench_function("simple_html", |b| {
        b.iter(|| {
            black_box(html_to_markdown(SIMPLE_HTML));
        });
    });

    // Medium HTML
    group.throughput(Throughput::Bytes(MEDIUM_HTML.len() as u64));
    group.bench_function("medium_html", |b| {
        b.iter(|| {
            black_box(html_to_markdown(MEDIUM_HTML));
        });
    });

    // Large HTML
    let large_html = generate_large_html(100, 500);
    group.throughput(Throughput::Bytes(large_html.len() as u64));
    group.bench_with_input(BenchmarkId::new("large_html", "100p"), &large_html, |b, html| {
        b.iter(|| {
            black_box(html_to_markdown(html));
        });
    });

    group.finish();
}

// =============================================================================
// CONTENT EXTRACTION BENCHMARKS
// =============================================================================

fn bench_content_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_extraction");

    // Medium HTML
    group.throughput(Throughput::Bytes(MEDIUM_HTML.len() as u64));
    group.bench_function("medium_html", |b| {
        b.iter(|| {
            black_box(extract_content(MEDIUM_HTML));
        });
    });

    // Large HTML
    let large_html = generate_large_html(100, 500);
    group.throughput(Throughput::Bytes(large_html.len() as u64));
    group.bench_with_input(BenchmarkId::new("large_html", "100p"), &large_html, |b, html| {
        b.iter(|| {
            black_box(extract_content(html));
        });
    });

    // Very large HTML
    let very_large_html = generate_large_html(500, 2000);
    group.throughput(Throughput::Bytes(very_large_html.len() as u64));
    group.bench_with_input(BenchmarkId::new("very_large_html", "500p"), &very_large_html, |b, html| {
        b.iter(|| {
            black_box(extract_content(html));
        });
    });

    group.finish();
}

// =============================================================================
// LANGUAGE DETECTION BENCHMARKS
// =============================================================================

fn bench_language_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("language_detection");

    let long_en = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(100);
    let texts: Vec<(&str, &str)> = vec![
        ("short_en", "Hello world, this is a test."),
        ("medium_en", "The quick brown fox jumps over the lazy dog. This sentence contains every letter of the alphabet. It is commonly used for font testing and typing practice."),
        ("long_en", &long_en),
        ("short_fr", "Bonjour le monde, ceci est un test."),
        ("short_de", "Hallo Welt, dies ist ein Test."),
        ("short_es", "Hola mundo, esto es una prueba."),
    ];

    for (name, text) in texts {
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &text, |b, text| {
            b.iter(|| {
                black_box(detect_language(text));
            });
        });
    }

    group.finish();
}

// =============================================================================
// THROUGHPUT ESTIMATION BENCHMARKS
// =============================================================================

fn bench_pages_per_second(c: &mut Criterion) {
    let mut group = c.benchmark_group("pages_per_second");
    group.sample_size(50);

    // Simulate processing many pages
    let pages: Vec<_> = (0..100)
        .map(|i| {
            let html = if i % 10 == 0 {
                generate_large_html(50, 200)
            } else {
                MEDIUM_HTML.to_string()
            };
            create_raw_page(&html)
        })
        .collect();

    group.throughput(Throughput::Elements(100));
    group.bench_function("batch_100_mixed", |b| {
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(true)
            .detect_language(true)
            .build();
        b.iter(|| {
            for page in &pages {
                black_box(parser.parse(page).unwrap());
            }
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    parsing_benches,
    bench_html_parsing,
    bench_full_parsing_pipeline,
);

criterion_group!(
    conversion_benches,
    bench_markdown_conversion,
    bench_content_extraction,
    bench_language_detection,
);

criterion_group!(
    throughput_benches,
    bench_pages_per_second,
);

criterion_main!(parsing_benches, conversion_benches, throughput_benches);
