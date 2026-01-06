//! End-to-End Wikipedia Indexing Benchmark
//!
//! This benchmark simulates indexing 100k Wikipedia-like pages to Meilisearch.
//!
//! Run with:
//!   cargo bench -p scrapix-benchmarks -- "wikipedia"
//!
//! For a more realistic test with actual Meilisearch:
//!   MEILISEARCH_URL=http://localhost:7700 MEILISEARCH_API_KEY=masterKey \
//!   cargo run --release --example wikipedia_e2e
//!
//! Performance targets based on architecture goals:
//! - Phase 1 (MVP): 1M pages/day = ~12 pages/second
//! - Phase 2 (Growth): 10M pages/day = ~116 pages/second
//! - Phase 3 (Scale): 100M pages/day = ~1,157 pages/second

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::distributions::Alphanumeric;
use rand::Rng;

use scrapix_core::{CrawlUrl, Document, RawPage};
use scrapix_frontier::{PriorityQueue, UrlDedup};
use scrapix_parser::HtmlParserBuilder;

// =============================================================================
// WIKIPEDIA PAGE GENERATION
// =============================================================================

/// Generate Wikipedia-like article content
/// Average Wikipedia article: ~3-5KB of text, 50-100 links
fn generate_wikipedia_article(title: &str, category: &str, word_count: usize) -> String {
    let mut rng = rand::thread_rng();

    // Generate realistic-looking paragraphs
    let paragraphs: Vec<String> = (0..word_count / 100)
        .map(|i| {
            let words: Vec<String> = (0..100)
                .map(|_| {
                    let len = rng.gen_range(3..12);
                    (0..len).map(|_| rng.sample(Alphanumeric) as char).collect()
                })
                .collect();
            if i % 5 == 0 {
                format!("<h2>{}</h2><p>{}</p>", words[0], words.join(" "))
            } else {
                format!("<p>{}</p>", words.join(" "))
            }
        })
        .collect();

    // Generate internal links
    let links: Vec<String> = (0..rng.gen_range(30..80))
        .map(|i| {
            let link_title: String = (0..rng.gen_range(5..20))
                .map(|_| rng.sample(Alphanumeric) as char)
                .collect();
            format!(r#"<a href="/wiki/{}">{}</a>"#, link_title, link_title)
        })
        .collect();

    // Generate categories
    let categories: Vec<String> = (0..rng.gen_range(3..10))
        .map(|_| {
            let cat: String = (0..rng.gen_range(5..15))
                .map(|_| rng.sample(Alphanumeric) as char)
                .collect();
            format!(r#"<a href="/wiki/Category:{}">{}</a>"#, cat, cat)
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <title>{title} - Wikipedia</title>
    <meta name="description" content="Wikipedia article about {title}">
    <meta property="og:title" content="{title}">
    <meta property="og:type" content="article">
    <meta property="article:section" content="{category}">
</head>
<body>
    <header>
        <nav id="mw-navigation">
            <a href="/wiki/Main_Page">Main page</a>
            <a href="/wiki/Special:Random">Random article</a>
        </nav>
    </header>
    <main id="content" role="main">
        <article id="mw-content-text">
            <h1 id="firstHeading">{title}</h1>
            <div class="mw-parser-output">
                {paragraphs}
                <div id="see-also">
                    <h2>See also</h2>
                    {links}
                </div>
            </div>
        </article>
        <div id="catlinks">
            <h3>Categories</h3>
            {categories}
        </div>
    </main>
    <footer>
        <p>This page was last edited on {date}</p>
    </footer>
</body>
</html>"#,
        title = title,
        category = category,
        paragraphs = paragraphs.join("\n"),
        links = links.join(" "),
        categories = categories.join(" "),
        date = Utc::now().format("%Y-%m-%d"),
    )
}

/// Generate a batch of Wikipedia-like pages
fn generate_wikipedia_batch(count: usize) -> Vec<(String, RawPage)> {
    let mut rng = rand::thread_rng();
    let categories = [
        "Science",
        "Technology",
        "History",
        "Geography",
        "Arts",
        "Sports",
        "Politics",
        "Biography",
    ];

    (0..count)
        .map(|i| {
            // Generate realistic word counts (Wikipedia average is ~600-3000 words)
            let word_count = rng.gen_range(300..2000);
            let category = categories[i % categories.len()];
            let title: String = (0..rng.gen_range(10..30))
                .map(|_| rng.sample(Alphanumeric) as char)
                .collect();

            let url = format!("https://en.wikipedia.org/wiki/{}", title);
            let html = generate_wikipedia_article(&title, category, word_count);

            let page = RawPage {
                url: url.clone(),
                final_url: url.clone(),
                status: 200,
                headers: HashMap::new(),
                html,
                content_type: Some("text/html".to_string()),
                js_rendered: false,
                fetched_at: Utc::now(),
                fetch_duration_ms: rng.gen_range(50..500),
            };

            (url, page)
        })
        .collect()
}

// =============================================================================
// BENCHMARKS
// =============================================================================

/// Benchmark: Parse Wikipedia-like pages
fn bench_wikipedia_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("wikipedia_parsing");
    group.sample_size(20);

    // Generate 1000 pages for testing
    let pages = generate_wikipedia_batch(1000);

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .convert_to_markdown(true)
        .detect_language(true)
        .build();

    // Calculate total bytes
    let total_bytes: usize = pages.iter().map(|(_, p)| p.html.len()).sum();

    group.bench_function("parse_1000_pages", |b| {
        b.iter(|| {
            for (_, page) in &pages {
                black_box(parser.parse(page).unwrap());
            }
        });
    });

    // Report throughput
    println!(
        "\n[Wikipedia Parsing Stats]"
    );
    println!("  Total pages: 1,000");
    println!("  Total HTML: {:.2} MB", total_bytes as f64 / 1_000_000.0);
    println!("  Avg page size: {:.2} KB", total_bytes as f64 / 1000.0 / 1000.0);

    group.finish();
}

/// Benchmark: Full pipeline simulation (URL dedup + Parse + Document creation)
fn bench_wikipedia_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("wikipedia_full_pipeline");
    group.sample_size(10);

    for page_count in [1_000, 10_000].iter() {
        let pages = generate_wikipedia_batch(*page_count);
        let urls: Vec<_> = pages.iter().map(|(url, _)| url.clone()).collect();

        group.bench_with_input(
            BenchmarkId::new("pages", page_count),
            &pages,
            |b, pages| {
                let dedup = UrlDedup::for_capacity(*page_count * 2, 0.01);
                let queue = PriorityQueue::with_defaults();
                let parser = HtmlParserBuilder::new()
                    .extract_content(true)
                    .convert_to_markdown(true)
                    .detect_language(true)
                    .build();

                b.iter(|| {
                    // Phase 1: URL deduplication
                    for (i, (url, _)) in pages.iter().enumerate() {
                        if !dedup.check_and_mark(url) {
                            queue.push(CrawlUrl::new(url.clone(), (i % 5) as u32));
                        }
                    }

                    // Phase 2: Parse pages and create documents
                    let mut documents = Vec::with_capacity(pages.len());
                    for (url, page) in pages {
                        if let Ok(parsed) = parser.parse(page) {
                            let mut doc = Document::new(url, "en.wikipedia.org");
                            doc.title = parsed.title;
                            doc.content = parsed.content;
                            doc.markdown = parsed.markdown;
                            doc.language = parsed.language;
                            documents.push(doc);
                        }
                    }

                    black_box(documents);
                    dedup.clear();
                    queue.clear();
                });
            },
        );
    }

    group.finish();
}

/// Estimate time to process 100k Wikipedia pages
fn bench_wikipedia_100k_estimate(c: &mut Criterion) {
    let mut group = c.benchmark_group("wikipedia_100k_estimate");
    group.sample_size(10);

    // Generate 10k pages (we'll extrapolate to 100k)
    let pages = generate_wikipedia_batch(10_000);

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .convert_to_markdown(true)
        .detect_language(true)
        .build();

    // Measure single-threaded throughput
    group.bench_function("baseline_10k", |b| {
        b.iter(|| {
            let dedup = UrlDedup::for_capacity(20_000, 0.01);
            let queue = PriorityQueue::with_defaults();

            for (i, (url, page)) in pages.iter().enumerate() {
                if !dedup.check_and_mark(url) {
                    queue.push(CrawlUrl::new(url.clone(), (i % 5) as u32));
                }
                black_box(parser.parse(page).unwrap());
            }
        });
    });

    // Measure multi-threaded throughput (simulating worker pool)
    group.bench_function("parallel_10k_4_threads", |b| {
        b.iter(|| {
            let dedup = Arc::new(UrlDedup::for_capacity(20_000, 0.01));
            let parsed_count = Arc::new(AtomicU64::new(0));

            let chunk_size = pages.len() / 4;
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let pages_chunk: Vec<_> = pages[t * chunk_size..(t + 1) * chunk_size]
                        .iter()
                        .cloned()
                        .collect();
                    let dedup = Arc::clone(&dedup);
                    let parsed_count = Arc::clone(&parsed_count);
                    let parser = HtmlParserBuilder::new()
                        .extract_content(true)
                        .convert_to_markdown(true)
                        .detect_language(true)
                        .build();

                    std::thread::spawn(move || {
                        for (url, page) in pages_chunk {
                            if !dedup.check_and_mark(&url) {
                                if parser.parse(&page).is_ok() {
                                    parsed_count.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }

            black_box(parsed_count.load(Ordering::Relaxed));
        });
    });

    group.finish();

    // Print estimation summary
    println!("\n============================================");
    println!("WIKIPEDIA 100K PAGE INDEXING ESTIMATION");
    println!("============================================");
    println!("\nBased on benchmark results, estimate for 100k pages:");
    println!("  - Single-threaded: ~10x baseline_10k time");
    println!("  - 4 threads: ~10x parallel_10k_4_threads time");
    println!("\nNote: Actual Meilisearch indexing adds overhead.");
    println!("Run with real Meilisearch for accurate E2E timing.");
    println!("============================================\n");
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    wikipedia_benches,
    bench_wikipedia_parsing,
    bench_wikipedia_full_pipeline,
    bench_wikipedia_100k_estimate,
);

criterion_main!(wikipedia_benches);
