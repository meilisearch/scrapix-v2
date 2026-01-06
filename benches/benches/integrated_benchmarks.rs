//! Integrated Performance Benchmarks for Scrapix
//!
//! Run with: cargo bench -p scrapix-benchmarks
//!
//! These benchmarks simulate real-world crawling scenarios:
//! - Full pipeline: URL dedup -> Parse -> Extract
//! - Throughput estimation for different workloads
//! - Memory efficiency analysis

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::distributions::Alphanumeric;
use rand::Rng;

use scrapix_core::{CrawlUrl, RawPage};
use scrapix_frontier::{PriorityQueue, UrlDedup};
use scrapix_parser::HtmlParserBuilder;

// =============================================================================
// TEST DATA GENERATION
// =============================================================================

/// Generate random URLs
fn generate_urls(count: usize, domain_count: usize) -> Vec<String> {
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|_| {
            let domain_id = rng.gen_range(0..domain_count);
            let path: String = (0..20).map(|_| rng.sample(Alphanumeric) as char).collect();
            format!("https://domain{}.example.com/{}", domain_id, path)
        })
        .collect()
}

/// Generate a realistic HTML page
fn generate_html_page(paragraph_count: usize, link_count: usize) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <title>Test Page</title>
    <meta name="description" content="A test page for benchmarking">
    <meta property="og:title" content="Test Page">
</head>
<body>
    <header><nav><a href="/">Home</a><a href="/about">About</a></nav></header>
    <main><article><h1>Test Article</h1>
"#,
    );

    for i in 0..paragraph_count {
        html.push_str(&format!(
            "<h2>Section {}</h2><p>This is paragraph {} with realistic content. \
             Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do \
             eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>\n",
            i, i
        ));
    }

    for i in 0..link_count {
        html.push_str(&format!(r#"<a href="/page{}">Link {}</a>"#, i, i));
    }

    html.push_str("</article></main><footer><p>Copyright 2024</p></footer></body></html>");
    html
}

/// Create a RawPage for benchmarking
fn create_raw_page(url: &str, html: &str) -> RawPage {
    RawPage {
        url: url.to_string(),
        final_url: url.to_string(),
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
// FULL PIPELINE BENCHMARKS
// =============================================================================

/// Benchmark the complete URL processing pipeline
fn bench_url_processing_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("url_processing_pipeline");

    for url_count in [1_000, 10_000, 100_000].iter() {
        let urls = generate_urls(*url_count, 100);

        group.throughput(Throughput::Elements(*url_count as u64));
        group.bench_with_input(
            BenchmarkId::new("dedup_queue", url_count),
            &urls,
            |b, urls| {
                let dedup = UrlDedup::for_capacity(*url_count * 2, 0.01);
                let queue = PriorityQueue::with_defaults();

                b.iter(|| {
                    for (i, url) in urls.iter().enumerate() {
                        if !dedup.check_and_mark(url) {
                            queue.push(CrawlUrl::new(url.clone(), (i % 10) as u32));
                        }
                    }

                    // Drain queue
                    while let Some(_url) = queue.pop() {
                        black_box(_url);
                    }

                    dedup.clear();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark page parsing throughput
fn bench_page_parsing_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("page_parsing_pipeline");

    // Generate test pages of different sizes
    let small_pages: Vec<_> = (0..100)
        .map(|i| {
            let html = generate_html_page(5, 10);
            create_raw_page(&format!("https://example.com/small/{}", i), &html)
        })
        .collect();

    let medium_pages: Vec<_> = (0..100)
        .map(|i| {
            let html = generate_html_page(20, 50);
            create_raw_page(&format!("https://example.com/medium/{}", i), &html)
        })
        .collect();

    let large_pages: Vec<_> = (0..100)
        .map(|i| {
            let html = generate_html_page(100, 200);
            create_raw_page(&format!("https://example.com/large/{}", i), &html)
        })
        .collect();

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .convert_to_markdown(true)
        .detect_language(true)
        .build();

    // Small pages
    group.throughput(Throughput::Elements(100));
    group.bench_function("small_pages_100", |b| {
        b.iter(|| {
            for page in &small_pages {
                black_box(parser.parse(page).unwrap());
            }
        });
    });

    // Medium pages
    group.bench_function("medium_pages_100", |b| {
        b.iter(|| {
            for page in &medium_pages {
                black_box(parser.parse(page).unwrap());
            }
        });
    });

    // Large pages
    group.bench_function("large_pages_100", |b| {
        b.iter(|| {
            for page in &large_pages {
                black_box(parser.parse(page).unwrap());
            }
        });
    });

    group.finish();
}

// =============================================================================
// CONCURRENT PROCESSING BENCHMARKS
// =============================================================================

/// Benchmark concurrent URL deduplication with multiple workers
fn bench_concurrent_url_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_url_processing");
    group.sample_size(20);

    for thread_count in [2, 4, 8, 16].iter() {
        let urls_per_thread = 10_000;
        let total_urls = urls_per_thread * *thread_count;

        // Pre-generate URLs for each thread
        let thread_urls: Vec<_> = (0..*thread_count)
            .map(|_| generate_urls(urls_per_thread, 100))
            .collect();

        group.throughput(Throughput::Elements(total_urls as u64));
        group.bench_with_input(
            BenchmarkId::new("threads", thread_count),
            &thread_urls,
            |b, thread_urls| {
                b.iter(|| {
                    let dedup = Arc::new(UrlDedup::for_capacity(total_urls * 2, 0.01));
                    let queue = Arc::new(PriorityQueue::with_defaults());

                    let handles: Vec<_> = thread_urls
                        .iter()
                        .enumerate()
                        .map(|(tid, urls)| {
                            let dedup = Arc::clone(&dedup);
                            let queue = Arc::clone(&queue);
                            let urls = urls.clone();

                            thread::spawn(move || {
                                for (i, url) in urls.iter().enumerate() {
                                    if !dedup.check_and_mark(url) {
                                        queue.push(CrawlUrl::new(
                                            url.clone(),
                                            ((tid * 1000 + i) % 10) as u32,
                                        ));
                                    }
                                }
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.join().unwrap();
                    }

                    dedup.clear();
                    queue.clear();
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// THROUGHPUT ESTIMATION
// =============================================================================

/// Estimate realistic crawling throughput
fn bench_throughput_estimation(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_estimation");
    group.sample_size(20);

    // Simulate crawling 1000 pages with realistic mix
    let pages: Vec<_> = (0..1000)
        .map(|i| {
            let html = if i % 10 == 0 {
                // 10% large pages
                generate_html_page(100, 200)
            } else if i % 3 == 0 {
                // 30% medium pages
                generate_html_page(30, 60)
            } else {
                // 60% small pages
                generate_html_page(10, 20)
            };
            create_raw_page(&format!("https://example.com/page/{}", i), &html)
        })
        .collect();

    let urls: Vec<_> = pages.iter().map(|p| p.url.clone()).collect();

    group.throughput(Throughput::Elements(1000));

    // Full pipeline: dedup + parse with all features
    group.bench_function("full_pipeline_1000_pages", |b| {
        let dedup = UrlDedup::for_capacity(10_000, 0.01);
        let queue = PriorityQueue::with_defaults();
        let parser = HtmlParserBuilder::new()
            .extract_content(true)
            .convert_to_markdown(true)
            .detect_language(true)
            .build();

        b.iter(|| {
            // Phase 1: URL deduplication
            for (i, url) in urls.iter().enumerate() {
                if !dedup.check_and_mark(url) {
                    queue.push(CrawlUrl::new(url.clone(), (i % 5) as u32));
                }
            }

            // Phase 2: Parse pages
            for page in &pages {
                black_box(parser.parse(page).unwrap());
            }

            dedup.clear();
            queue.clear();
        });
    });

    // Parse only (to measure parsing overhead)
    group.bench_function("parse_only_1000_pages", |b| {
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

    // Dedup only (to measure dedup overhead)
    group.bench_function("dedup_only_1000_urls", |b| {
        let dedup = UrlDedup::for_capacity(10_000, 0.01);
        let queue = PriorityQueue::with_defaults();

        b.iter(|| {
            for (i, url) in urls.iter().enumerate() {
                if !dedup.check_and_mark(url) {
                    queue.push(CrawlUrl::new(url.clone(), (i % 5) as u32));
                }
            }
            dedup.clear();
            queue.clear();
        });
    });

    group.finish();
}

// =============================================================================
// MEMORY USAGE ESTIMATION
// =============================================================================

fn bench_memory_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_scaling");
    group.sample_size(10);

    // Test bloom filter memory efficiency at different scales
    for capacity in [100_000, 1_000_000, 10_000_000, 100_000_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("bloom_filter_create", capacity),
            capacity,
            |b, &capacity| {
                b.iter(|| {
                    let dedup = UrlDedup::for_capacity(capacity, 0.01);
                    let stats = dedup.stats();
                    black_box((dedup, stats));
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    pipeline_benches,
    bench_url_processing_pipeline,
    bench_page_parsing_pipeline,
);

criterion_group!(concurrent_benches, bench_concurrent_url_processing,);

criterion_group!(
    estimation_benches,
    bench_throughput_estimation,
    bench_memory_scaling,
);

criterion_main!(pipeline_benches, concurrent_benches, estimation_benches);
