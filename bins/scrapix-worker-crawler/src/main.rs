//! Scrapix Crawler Worker
//!
//! Distributed worker that fetches web pages from the URL frontier queue.
//!
//! ## Responsibilities
//!
//! 1. Consume URLs from the frontier topic
//! 2. Fetch pages (respecting robots.txt and rate limits)
//! 3. Extract links from fetched pages
//! 4. Publish raw pages to the content processing topic
//! 5. Publish discovered URLs back to the frontier

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use scrapix_core::ScrapixError;
use scrapix_crawler::{
    ExtractorConfig, HttpFetcher, HttpFetcherBuilder, RobotsCache, RobotsConfig, UrlExtractor,
};
use scrapix_queue::{
    topic_names, ConsumerBuilder, CrawlEvent, KafkaConsumer, KafkaProducer, ProducerBuilder,
    RawPageMessage, UrlMessage,
};

/// Crawler worker for fetching web pages from the URL frontier
#[derive(Parser, Debug)]
#[command(name = "scrapix-worker-crawler")]
#[command(version, about = "Crawler worker for fetching web pages")]
struct Args {
    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    brokers: String,

    /// Consumer group ID
    #[arg(
        short,
        long,
        env = "KAFKA_GROUP_ID",
        default_value = "scrapix-crawlers"
    )]
    group_id: String,

    /// Number of concurrent fetchers
    #[arg(short, long, env = "CONCURRENCY", default_value = "10")]
    concurrency: usize,

    /// User agent string
    #[arg(
        long,
        env = "USER_AGENT",
        default_value = "Scrapix/1.0 (compatible; +https://github.com/quentindequelen/scrapix)"
    )]
    user_agent: String,

    /// Request timeout in seconds
    #[arg(long, env = "REQUEST_TIMEOUT", default_value = "30")]
    timeout: u64,

    /// Maximum retries per URL
    #[arg(long, env = "MAX_RETRIES", default_value = "3")]
    max_retries: u32,

    /// Follow external links (different domain)
    #[arg(long, env = "FOLLOW_EXTERNAL")]
    follow_external: bool,

    /// Maximum crawl depth
    #[arg(long, env = "MAX_DEPTH", default_value = "100")]
    max_depth: u32,

    /// Maximum response body size in MB
    #[arg(long, env = "MAX_BODY_SIZE_MB", default_value = "10")]
    max_body_size_mb: usize,

    /// Respect robots.txt
    #[arg(long, env = "RESPECT_ROBOTS", default_value = "true")]
    respect_robots: bool,

    /// Worker ID (for logging/metrics)
    #[arg(long, env = "WORKER_ID")]
    worker_id: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// Worker metrics for monitoring
#[derive(Debug, Default)]
struct WorkerMetrics {
    urls_processed: AtomicU64,
    urls_succeeded: AtomicU64,
    urls_failed: AtomicU64,
    urls_discovered: AtomicU64,
    bytes_downloaded: AtomicU64,
    active_fetches: AtomicU64,
}

impl WorkerMetrics {
    fn new() -> Self {
        Self::default()
    }

    fn record_success(&self, bytes: u64) {
        self.urls_processed.fetch_add(1, Ordering::Relaxed);
        self.urls_succeeded.fetch_add(1, Ordering::Relaxed);
        self.bytes_downloaded.fetch_add(bytes, Ordering::Relaxed);
    }

    fn record_failure(&self) {
        self.urls_processed.fetch_add(1, Ordering::Relaxed);
        self.urls_failed.fetch_add(1, Ordering::Relaxed);
    }

    fn record_discovered(&self, count: u64) {
        self.urls_discovered.fetch_add(count, Ordering::Relaxed);
    }

    fn fetch_started(&self) {
        self.active_fetches.fetch_add(1, Ordering::Relaxed);
    }

    fn fetch_completed(&self) {
        self.active_fetches.fetch_sub(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            urls_processed: self.urls_processed.load(Ordering::Relaxed),
            urls_succeeded: self.urls_succeeded.load(Ordering::Relaxed),
            urls_failed: self.urls_failed.load(Ordering::Relaxed),
            urls_discovered: self.urls_discovered.load(Ordering::Relaxed),
            bytes_downloaded: self.bytes_downloaded.load(Ordering::Relaxed),
            active_fetches: self.active_fetches.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct MetricsSnapshot {
    urls_processed: u64,
    urls_succeeded: u64,
    urls_failed: u64,
    urls_discovered: u64,
    bytes_downloaded: u64,
    active_fetches: u64,
}

/// The main crawler worker
struct CrawlerWorker {
    consumer: KafkaConsumer,
    producer: KafkaProducer,
    fetcher: HttpFetcher,
    extractor: UrlExtractor,
    semaphore: Arc<Semaphore>,
    metrics: Arc<WorkerMetrics>,
    shutdown: Arc<AtomicBool>,
    worker_id: String,
}

impl CrawlerWorker {
    /// Create a new crawler worker
    async fn new(args: &Args) -> anyhow::Result<Self> {
        let worker_id = args
            .worker_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(worker_id = %worker_id, "Initializing crawler worker");

        // Create Kafka consumer
        let consumer = ConsumerBuilder::new(&args.brokers, &args.group_id)
            .client_id(format!("scrapix-crawler-{}", worker_id))
            .auto_offset_reset("earliest")
            .build()?;

        // Subscribe to processing topic (URLs ready to crawl after dedup/politeness)
        consumer.subscribe(&[topic_names::URL_PROCESSING])?;
        info!(
            topic = topic_names::URL_PROCESSING,
            "Subscribed to processing topic"
        );

        // Create Kafka producer
        let producer = ProducerBuilder::new(&args.brokers)
            .client_id(format!("scrapix-crawler-{}-producer", worker_id))
            .compression("lz4")
            .build()?;

        // Create robots.txt cache
        let robots_config = RobotsConfig {
            user_agent: args.user_agent.clone(),
            cache_ttl: Duration::from_secs(3600), // 1 hour
            fetch_timeout: Duration::from_secs(10),
            respect_robots: args.respect_robots,
            default_crawl_delay_ms: None,
        };
        let robots_cache = Arc::new(RobotsCache::new(robots_config)?);

        // Create HTTP fetcher
        let fetcher = HttpFetcherBuilder::new()
            .user_agent(&args.user_agent)
            .timeout(Duration::from_secs(args.timeout))
            .max_retries(args.max_retries)
            .max_body_size(args.max_body_size_mb * 1024 * 1024)
            .build(robots_cache)?;

        // Create URL extractor
        let extractor_config = ExtractorConfig {
            patterns: None,
            max_depth: args.max_depth,
            follow_external: args.follow_external,
            follow_subdomains: true,
            extract_from_data_attrs: false,
        };
        let extractor = UrlExtractor::new(extractor_config);

        // Create concurrency limiter
        let semaphore = Arc::new(Semaphore::new(args.concurrency));

        Ok(Self {
            consumer,
            producer,
            fetcher,
            extractor,
            semaphore,
            metrics: Arc::new(WorkerMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            worker_id,
        })
    }

    /// Run the crawler worker
    async fn run(&self) -> anyhow::Result<()> {
        info!(worker_id = %self.worker_id, "Starting crawler worker main loop");

        // Start metrics reporter
        let metrics = self.metrics.clone();
        let shutdown = self.shutdown.clone();
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            while !shutdown.load(Ordering::Relaxed) {
                interval.tick().await;
                let snapshot = metrics.snapshot();
                info!(
                    processed = snapshot.urls_processed,
                    succeeded = snapshot.urls_succeeded,
                    failed = snapshot.urls_failed,
                    discovered = snapshot.urls_discovered,
                    bytes_mb = snapshot.bytes_downloaded / (1024 * 1024),
                    active = snapshot.active_fetches,
                    "Worker metrics"
                );
            }
        });

        // Process messages
        let result = self.process_messages().await;

        // Cleanup
        self.shutdown.store(true, Ordering::Relaxed);
        metrics_handle.abort();

        result
    }

    /// Process messages from the frontier queue
    async fn process_messages(&self) -> anyhow::Result<()> {
        // Use the consumer's process method with our handler
        self.consumer
            .process::<UrlMessage, _, _>(|msg, metadata| async move {
                if self.shutdown.load(Ordering::Relaxed) {
                    return Err(ScrapixError::Crawl("Worker shutting down".into()));
                }

                debug!(
                    url = %msg.url.url,
                    job_id = %msg.job_id,
                    partition = metadata.partition,
                    offset = metadata.offset,
                    "Received URL from frontier"
                );

                // Acquire semaphore permit for concurrency limiting
                let _permit = self.semaphore.acquire().await.map_err(|e| {
                    ScrapixError::Crawl(format!("Failed to acquire semaphore: {}", e))
                })?;

                self.metrics.fetch_started();
                let result = self.process_url(&msg).await;
                self.metrics.fetch_completed();

                if let Err(ref e) = result {
                    warn!(
                        url = %msg.url.url,
                        job_id = %msg.job_id,
                        error = %e,
                        "Failed to process URL"
                    );
                }

                result
            })
            .await?;

        Ok(())
    }

    /// Process a single URL
    async fn process_url(&self, msg: &UrlMessage) -> scrapix_core::Result<()> {
        let start = Instant::now();
        let url = &msg.url;

        // Fetch the page
        let page = match self.fetcher.fetch(url).await {
            Ok(page) => page,
            Err(e) => {
                self.metrics.record_failure();

                // Send failure event
                let event =
                    CrawlEvent::page_failed(&msg.job_id, &url.url, e.to_string(), url.retry_count);
                self.publish_event(&msg.job_id, &event).await?;

                return Err(e);
            }
        };

        let fetch_duration = start.elapsed();
        let page_size = page.html.len() as u64;

        self.metrics.record_success(page_size);

        info!(
            url = %url.url,
            status = page.status,
            size_kb = page_size / 1024,
            duration_ms = fetch_duration.as_millis(),
            "Page fetched successfully"
        );

        // Extract URLs from the page
        let discovered_urls = self.extractor.extract(&page, url.depth);
        let discovered_count = discovered_urls.len();

        self.metrics.record_discovered(discovered_count as u64);

        debug!(
            url = %url.url,
            count = discovered_count,
            "Extracted URLs from page"
        );

        // Publish raw page to content processing topic
        let raw_page_msg = RawPageMessage {
            url: page.url.clone(),
            final_url: page.final_url.clone(),
            status: page.status,
            html: page.html,
            content_type: page.content_type,
            js_rendered: page.js_rendered,
            fetched_at: page.fetched_at.timestamp_millis(),
            fetch_duration_ms: page.fetch_duration_ms,
            job_id: msg.job_id.clone(),
            index_uid: msg.index_uid.clone(),
            message_id: uuid::Uuid::new_v4().to_string(),
        };

        self.producer
            .send(topic_names::PAGES_RAW, Some(&msg.job_id), &raw_page_msg)
            .await?;

        debug!(
            url = %page.url,
            topic = topic_names::PAGES_RAW,
            "Published raw page to content topic"
        );

        // Publish discovered URLs back to frontier
        for discovered_url in discovered_urls {
            let url_msg = UrlMessage::new(discovered_url.clone(), &msg.job_id, &msg.index_uid);

            self.producer
                .send(
                    topic_names::URL_FRONTIER,
                    Some(&url_msg.partition_key()),
                    &url_msg,
                )
                .await?;
        }

        if discovered_count > 0 {
            debug!(
                source_url = %url.url,
                count = discovered_count,
                topic = topic_names::URL_FRONTIER,
                "Published discovered URLs to frontier"
            );

            // Send URLs discovered event
            let event = CrawlEvent::UrlsDiscovered {
                job_id: msg.job_id.clone(),
                source_url: url.url.clone(),
                count: discovered_count,
                timestamp: chrono::Utc::now().timestamp_millis(),
            };
            self.publish_event(&msg.job_id, &event).await?;
        }

        // Send success event
        let event = CrawlEvent::page_crawled(
            &msg.job_id,
            &url.url,
            page.status,
            fetch_duration.as_millis() as u64,
        );
        self.publish_event(&msg.job_id, &event).await?;

        Ok(())
    }

    /// Publish a crawl event
    async fn publish_event(&self, job_id: &str, event: &CrawlEvent) -> scrapix_core::Result<()> {
        self.producer
            .send(topic_names::EVENTS, Some(job_id), event)
            .await?;
        Ok(())
    }

    /// Graceful shutdown
    fn shutdown(&self) {
        info!(worker_id = %self.worker_id, "Initiating graceful shutdown");
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();

    info!(
        concurrency = args.concurrency,
        brokers = %args.brokers,
        group_id = %args.group_id,
        "Starting Scrapix crawler worker"
    );

    // Create and run worker
    let worker = Arc::new(CrawlerWorker::new(&args).await?);

    // Setup shutdown handler
    let worker_shutdown = worker.clone();
    let shutdown_handle = tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!(error = %e, "Failed to listen for ctrl+c");
        }
        info!("Received shutdown signal");
        worker_shutdown.shutdown();
    });

    // Run the worker
    let result = worker.run().await;

    // Cleanup
    shutdown_handle.abort();

    // Print final metrics
    let metrics = worker.metrics.snapshot();
    info!(
        processed = metrics.urls_processed,
        succeeded = metrics.urls_succeeded,
        failed = metrics.urls_failed,
        discovered = metrics.urls_discovered,
        bytes_mb = metrics.bytes_downloaded / (1024 * 1024),
        "Final worker metrics"
    );

    result
}
