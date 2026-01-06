//! Performance benchmarks for Scrapix Frontier
//!
//! Run with: cargo bench -p scrapix-frontier
//!
//! These benchmarks measure:
//! - Bloom filter URL deduplication throughput
//! - Priority queue operations
//! - Partitioned deduplication at scale

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::distributions::Alphanumeric;
use rand::Rng;
use scrapix_frontier::{
    MultiLevelPriorityQueue, PartitionedUrlDedup, PriorityQueue, UrlDedup,
};

/// Generate random URLs for benchmarking
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

/// Generate CrawlUrl instances for queue benchmarks
fn generate_crawl_urls(count: usize) -> Vec<scrapix_core::CrawlUrl> {
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|i| {
            let depth = rng.gen_range(0..10);
            let priority = rng.gen_range(-5..15);
            scrapix_core::CrawlUrl::new(
                format!("https://example.com/page/{}", i),
                depth,
            )
            .with_priority(priority)
        })
        .collect()
}

// =============================================================================
// BLOOM FILTER DEDUPLICATION BENCHMARKS
// =============================================================================

fn bench_bloom_filter_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter_insert");

    for size in [1_000, 10_000, 100_000, 1_000_000].iter() {
        let urls = generate_urls(*size, 100);
        let dedup = UrlDedup::for_capacity(*size * 2, 0.01);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                for url in urls {
                    dedup.mark_seen(black_box(url));
                }
                dedup.clear();
            });
        });
    }

    group.finish();
}

fn bench_bloom_filter_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter_check");

    for size in [1_000, 10_000, 100_000, 1_000_000].iter() {
        let urls = generate_urls(*size, 100);
        let dedup = UrlDedup::for_capacity(*size * 2, 0.01);

        // Pre-populate the filter
        for url in &urls {
            dedup.mark_seen(url);
        }

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                for url in urls {
                    black_box(dedup.is_seen(url));
                }
            });
        });
    }

    group.finish();
}

fn bench_bloom_filter_check_and_mark(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter_check_and_mark");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_urls(*size, 100);
        let dedup = UrlDedup::for_capacity(*size * 2, 0.01);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                for url in urls {
                    black_box(dedup.check_and_mark(url));
                }
                dedup.clear();
            });
        });
    }

    group.finish();
}

fn bench_bloom_filter_batch_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter_batch_insert");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_urls(*size, 100);
        let dedup = UrlDedup::for_capacity(*size * 2, 0.01);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                dedup.mark_seen_batch(black_box(urls));
                dedup.clear();
            });
        });
    }

    group.finish();
}

fn bench_bloom_filter_filter_unseen(c: &mut Criterion) {
    let mut group = c.benchmark_group("bloom_filter_filter_unseen");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_urls(*size, 100);
        let dedup = UrlDedup::for_capacity(*size * 2, 0.01);

        // Pre-populate with half the URLs
        for url in urls.iter().take(*size / 2) {
            dedup.mark_seen(url);
        }

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                black_box(dedup.filter_unseen(urls.clone()));
            });
        });
    }

    group.finish();
}

// =============================================================================
// PARTITIONED BLOOM FILTER BENCHMARKS
// =============================================================================

fn bench_partitioned_bloom_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("partitioned_bloom_filter");

    for size in [10_000, 100_000, 1_000_000].iter() {
        let urls = generate_urls(*size, 100);

        // Compare regular vs partitioned
        group.throughput(Throughput::Elements(*size as u64));

        // Regular bloom filter
        let dedup_regular = UrlDedup::for_capacity(*size * 2, 0.01);
        group.bench_with_input(
            BenchmarkId::new("regular", size),
            &urls,
            |b, urls| {
                b.iter(|| {
                    for url in urls {
                        dedup_regular.check_and_mark(black_box(url));
                    }
                    dedup_regular.clear();
                });
            },
        );

        // Partitioned bloom filter (16 partitions)
        let dedup_partitioned = PartitionedUrlDedup::new(*size * 2, 0.01, 16);
        group.bench_with_input(
            BenchmarkId::new("partitioned_16", size),
            &urls,
            |b, urls| {
                b.iter(|| {
                    for url in urls {
                        dedup_partitioned.check_and_mark(black_box(url));
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// PRIORITY QUEUE BENCHMARKS
// =============================================================================

fn bench_priority_queue_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_push");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_crawl_urls(*size);
        let queue = PriorityQueue::with_defaults();

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                for url in urls {
                    queue.push(black_box(url.clone()));
                }
                queue.clear();
            });
        });
    }

    group.finish();
}

fn bench_priority_queue_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_pop");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_crawl_urls(*size);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter_batched(
                || {
                    let queue = PriorityQueue::with_defaults();
                    for url in urls {
                        queue.push(url.clone());
                    }
                    queue
                },
                |queue| {
                    while let Some(url) = queue.pop() {
                        black_box(url);
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_priority_queue_push_many(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_push_many");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_crawl_urls(*size);
        let queue = PriorityQueue::with_defaults();

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &urls, |b, urls| {
            b.iter(|| {
                queue.push_many(black_box(urls.clone()));
                queue.clear();
            });
        });
    }

    group.finish();
}

fn bench_priority_queue_pop_many(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue_pop_many");

    for batch_size in [100, 500, 1000].iter() {
        let urls = generate_crawl_urls(100_000);

        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &batch_size| {
                b.iter_batched(
                    || {
                        let queue = PriorityQueue::with_defaults();
                        queue.push_many(urls.clone());
                        queue
                    },
                    |queue| {
                        while queue.len() >= batch_size {
                            black_box(queue.pop_many(batch_size));
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// =============================================================================
// MULTI-LEVEL PRIORITY QUEUE BENCHMARKS
// =============================================================================

fn bench_multi_level_queue(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_level_queue");

    for size in [1_000, 10_000, 100_000].iter() {
        let urls = generate_crawl_urls(*size);

        group.throughput(Throughput::Elements(*size as u64));

        // Push benchmark
        group.bench_with_input(BenchmarkId::new("push", size), &urls, |b, urls| {
            let queue = MultiLevelPriorityQueue::with_defaults();
            b.iter(|| {
                for url in urls {
                    queue.push(black_box(url.clone()));
                }
            });
        });

        // Pop benchmark
        group.bench_with_input(BenchmarkId::new("pop", size), &urls, |b, urls| {
            b.iter_batched(
                || {
                    let queue = MultiLevelPriorityQueue::with_defaults();
                    for url in urls {
                        queue.push(url.clone());
                    }
                    queue
                },
                |queue| {
                    while let Some(url) = queue.pop() {
                        black_box(url);
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// =============================================================================
// MEMORY EFFICIENCY BENCHMARKS
// =============================================================================

fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");
    group.sample_size(10); // Fewer samples for memory benchmarks

    for capacity in [100_000, 1_000_000, 10_000_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("bloom_filter_create", capacity),
            capacity,
            |b, &capacity| {
                b.iter(|| {
                    black_box(UrlDedup::for_capacity(capacity, 0.01));
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// CONCURRENT ACCESS BENCHMARKS
// =============================================================================

fn bench_concurrent_dedup(c: &mut Criterion) {
    use std::sync::Arc;
    use std::thread;

    let mut group = c.benchmark_group("concurrent_dedup");
    group.sample_size(20);

    for thread_count in [2, 4, 8].iter() {
        let urls = Arc::new(generate_urls(10_000, 100));
        let dedup = Arc::new(UrlDedup::for_capacity(100_000, 0.01));

        group.throughput(Throughput::Elements(10_000 * *thread_count as u64));
        group.bench_with_input(
            BenchmarkId::new("check_and_mark", thread_count),
            thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let handles: Vec<_> = (0..thread_count)
                        .map(|_| {
                            let urls = Arc::clone(&urls);
                            let dedup = Arc::clone(&dedup);
                            thread::spawn(move || {
                                for url in urls.iter() {
                                    dedup.check_and_mark(url);
                                }
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.join().unwrap();
                    }
                    dedup.clear();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    bloom_filter_benches,
    bench_bloom_filter_insert,
    bench_bloom_filter_check,
    bench_bloom_filter_check_and_mark,
    bench_bloom_filter_batch_insert,
    bench_bloom_filter_filter_unseen,
    bench_partitioned_bloom_filter,
);

criterion_group!(
    priority_queue_benches,
    bench_priority_queue_push,
    bench_priority_queue_pop,
    bench_priority_queue_push_many,
    bench_priority_queue_pop_many,
    bench_multi_level_queue,
);

criterion_group!(
    scalability_benches,
    bench_memory_efficiency,
    bench_concurrent_dedup,
);

criterion_main!(bloom_filter_benches, priority_queue_benches, scalability_benches);
