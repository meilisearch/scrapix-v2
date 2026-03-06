//! Scrapix Crawler Worker
//!
//! Distributed worker that fetches web pages from the URL frontier queue.
//!
//! ## Responsibilities
//!
//! 1. Consume URLs from the frontier topic
//! 2. Fetch pages (respecting robots.txt and rate limits)
//! 3. Extract links from fetched pages
//! 4. Track link graph for priority boosting
//! 5. Publish raw pages to the content processing topic
//! 6. Publish discovered URLs back to the frontier
//!
//! ## Features
//!
//! - DNS caching for improved performance
//! - Link graph analysis for priority boosting
//! - Incremental crawling with conditional HTTP headers

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use scrapix_core::{CrawlUrl, FeaturesConfig, ScrapixError, UrlPatterns};
#[cfg(feature = "browser")]
use scrapix_crawler::{CdpRenderer, CdpRendererBuilder};
use scrapix_crawler::{
    ConditionalRequestHeaders, ExtractorConfig, HttpFetcher, HttpFetcherBuilder,
    PersistentRobotsCache, RobotsCache, RobotsConfig, RobotsPersistence, SitemapParser,
    UrlExtractor,
};
use scrapix_frontier::LinkGraph;
use scrapix_queue::{
    topic_names, AnyConsumer, AnyProducer, ConsumerBuilder, CrawlEvent, LinksMessage,
    ProducerBuilder, RawPageMessage, UrlMessage,
};
use scrapix_storage::{RocksConfig, RocksStorage, RocksStorageAdapter};

use std::collections::HashSet;

/// Adapter to implement RocksDbOps for RocksStorageAdapter
struct RocksRobotsPersistenceAdapter {
    storage: Arc<RocksStorageAdapter>,
}

impl RocksRobotsPersistenceAdapter {
    fn new(storage: Arc<RocksStorageAdapter>) -> Self {
        Self { storage }
    }
}

impl RobotsPersistence for RocksRobotsPersistenceAdapter {
    fn get(
        &self,
        domain: &str,
    ) -> scrapix_core::Result<Option<scrapix_crawler::PersistentRobotsEntry>> {
        let key = format!("robots:{}", domain).into_bytes();
        match self.storage.get_cf("robots_cache", &key)? {
            Some(data) => {
                let entry = serde_json::from_slice(&data)
                    .map_err(|e| ScrapixError::Storage(format!("Failed to deserialize: {}", e)))?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    fn put(
        &self,
        domain: &str,
        entry: &scrapix_crawler::PersistentRobotsEntry,
    ) -> scrapix_core::Result<()> {
        let key = format!("robots:{}", domain).into_bytes();
        let data = serde_json::to_vec(entry)
            .map_err(|e| ScrapixError::Storage(format!("Failed to serialize: {}", e)))?;
        self.storage.put_cf("robots_cache", &key, &data)
    }

    fn delete(&self, domain: &str) -> scrapix_core::Result<()> {
        let key = format!("robots:{}", domain).into_bytes();
        self.storage.delete_cf("robots_cache", &key)
    }

    fn list_domains(&self) -> scrapix_core::Result<Vec<String>> {
        let prefix = b"robots:";
        let entries = self.storage.prefix_iter_cf("robots_cache", prefix)?;
        let domains = entries
            .into_iter()
            .filter_map(|(key, _)| {
                String::from_utf8(key)
                    .ok()
                    .and_then(|s| s.strip_prefix("robots:").map(|d| d.to_string()))
            })
            .collect();
        Ok(domains)
    }

    fn count(&self) -> scrapix_core::Result<usize> {
        Ok(self.list_domains()?.len())
    }

    fn clear(&self) -> scrapix_core::Result<()> {
        for domain in self.list_domains()? {
            self.delete(&domain)?;
        }
        Ok(())
    }
}

/// Crawler worker for fetching web pages from the URL frontier
#[derive(Parser, Debug)]
#[command(name = "scrapix-worker-crawler")]
#[command(version, about = "Crawler worker for fetching web pages")]
pub struct Args {
    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    pub brokers: String,

    /// Consumer group ID
    #[arg(
        short,
        long,
        env = "KAFKA_GROUP_ID",
        default_value = "scrapix-crawlers"
    )]
    pub group_id: String,

    /// Number of concurrent fetchers
    #[arg(short, long, env = "CONCURRENCY", default_value = "50")]
    pub concurrency: usize,

    /// User agent string
    #[arg(
        long,
        env = "USER_AGENT",
        default_value = "Scrapix/1.0 (compatible; +https://github.com/quentindequelen/scrapix)"
    )]
    pub user_agent: String,

    /// Request timeout in seconds
    #[arg(long, env = "REQUEST_TIMEOUT", default_value = "30")]
    pub timeout: u64,

    /// Maximum retries per URL
    #[arg(long, env = "MAX_RETRIES", default_value = "3")]
    pub max_retries: u32,

    /// Follow external links (different domain)
    #[arg(long, env = "FOLLOW_EXTERNAL")]
    pub follow_external: bool,

    /// Maximum crawl depth
    #[arg(long, env = "MAX_DEPTH", default_value = "100")]
    pub max_depth: u32,

    /// Maximum response body size in MB
    #[arg(long, env = "MAX_BODY_SIZE_MB", default_value = "10")]
    pub max_body_size_mb: usize,

    /// Respect robots.txt
    #[arg(long, env = "RESPECT_ROBOTS", default_value = "true")]
    pub respect_robots: bool,

    /// Worker ID (for logging/metrics)
    #[arg(long, env = "WORKER_ID")]
    pub worker_id: Option<String>,

    /// Enable DNS caching for improved performance
    #[arg(long, env = "DNS_CACHE", default_value = "true")]
    pub dns_cache: bool,

    /// DNS cache TTL in seconds
    #[arg(long, env = "DNS_CACHE_TTL", default_value = "300")]
    pub dns_cache_ttl: u64,

    /// Enable link graph tracking for priority boosting
    #[arg(long, env = "LINK_GRAPH", default_value = "true")]
    pub link_graph: bool,

    /// Link graph score computation interval (in URLs processed)
    #[arg(long, env = "LINK_GRAPH_INTERVAL", default_value = "1000")]
    pub link_graph_interval: u64,

    /// Publish link data to frontier service for centralized PageRank
    #[arg(long, env = "PUBLISH_LINKS", default_value = "false")]
    pub publish_links: bool,

    /// Enable incremental crawling (use conditional HTTP headers)
    #[arg(long, env = "INCREMENTAL_CRAWL", default_value = "true")]
    pub incremental_crawl: bool,

    /// Enable browser rendering for JavaScript-heavy pages (requires --features browser)
    #[arg(long, env = "BROWSER_RENDER")]
    pub browser_render: bool,

    /// URL patterns that require browser rendering (regex, comma-separated)
    /// Example: ".*spa\.example\.com.*,.*react-app.*"
    #[arg(long, env = "BROWSER_RENDER_PATTERNS")]
    pub browser_render_patterns: Option<String>,

    /// Chrome/Chromium executable path for browser rendering
    #[arg(long, env = "CHROME_PATH")]
    pub chrome_path: Option<String>,

    /// Browser rendering timeout in seconds
    #[arg(long, env = "BROWSER_TIMEOUT", default_value = "30")]
    pub browser_timeout: u64,

    /// Maximum concurrent browser pages
    #[arg(long, env = "BROWSER_CONCURRENCY", default_value = "5")]
    pub browser_concurrency: usize,

    /// Run browser in headless mode
    #[arg(long, env = "BROWSER_HEADLESS", default_value = "true")]
    pub browser_headless: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// RocksDB path for persistent state (robots.txt cache, etc.)
    #[arg(long, env = "ROCKSDB_PATH", default_value = "./data/crawler-rocksdb")]
    pub rocksdb_path: String,

    /// Enable sitemap discovery from robots.txt
    #[arg(long, env = "SITEMAP_DISCOVERY", default_value = "true")]
    pub sitemap_discovery: bool,

    /// Maximum sitemap URLs to discover per domain
    #[arg(long, env = "MAX_SITEMAP_URLS", default_value = "10000")]
    pub max_sitemap_urls: usize,
}

/// Worker metrics for monitoring
#[derive(Debug, Default)]
struct WorkerMetrics {
    urls_processed: AtomicU64,
    urls_succeeded: AtomicU64,
    urls_failed: AtomicU64,
    urls_discovered: AtomicU64,
    urls_not_modified: AtomicU64,
    bytes_downloaded: AtomicU64,
    active_fetches: AtomicU64,
    dns_cache_hits: AtomicU64,
    dns_cache_misses: AtomicU64,
    browser_renders: AtomicU64,
    http_fetches: AtomicU64,
    sitemap_urls_discovered: AtomicU64,
    domains_with_sitemaps: AtomicU64,
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

    fn record_not_modified(&self) {
        self.urls_processed.fetch_add(1, Ordering::Relaxed);
        self.urls_not_modified.fetch_add(1, Ordering::Relaxed);
    }

    fn record_discovered(&self, count: u64) {
        self.urls_discovered.fetch_add(count, Ordering::Relaxed);
    }

    fn record_dns_stats(&self, hits: u64, misses: u64) {
        self.dns_cache_hits.store(hits, Ordering::Relaxed);
        self.dns_cache_misses.store(misses, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn record_browser_render(&self) {
        self.browser_renders.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn record_http_fetch(&self) {
        self.http_fetches.fetch_add(1, Ordering::Relaxed);
    }

    fn record_sitemap_discovery(&self, urls_count: u64) {
        self.sitemap_urls_discovered
            .fetch_add(urls_count, Ordering::Relaxed);
        self.domains_with_sitemaps.fetch_add(1, Ordering::Relaxed);
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
            urls_not_modified: self.urls_not_modified.load(Ordering::Relaxed),
            bytes_downloaded: self.bytes_downloaded.load(Ordering::Relaxed),
            active_fetches: self.active_fetches.load(Ordering::Relaxed),
            dns_cache_hits: self.dns_cache_hits.load(Ordering::Relaxed),
            dns_cache_misses: self.dns_cache_misses.load(Ordering::Relaxed),
            browser_renders: self.browser_renders.load(Ordering::Relaxed),
            http_fetches: self.http_fetches.load(Ordering::Relaxed),
            sitemap_urls_discovered: self.sitemap_urls_discovered.load(Ordering::Relaxed),
            domains_with_sitemaps: self.domains_with_sitemaps.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct MetricsSnapshot {
    urls_processed: u64,
    urls_succeeded: u64,
    urls_failed: u64,
    urls_discovered: u64,
    urls_not_modified: u64,
    bytes_downloaded: u64,
    active_fetches: u64,
    dns_cache_hits: u64,
    dns_cache_misses: u64,
    browser_renders: u64,
    http_fetches: u64,
    #[allow(dead_code)]
    sitemap_urls_discovered: u64,
    #[allow(dead_code)]
    domains_with_sitemaps: u64,
}

/// The main crawler worker
struct CrawlerWorker {
    consumer: AnyConsumer,
    producer: AnyProducer,
    fetcher: HttpFetcher,
    robots_cache: Arc<PersistentRobotsCache>,
    sitemap_parser: Option<SitemapParser>,
    #[cfg(feature = "browser")]
    browser_renderer: Option<Arc<CdpRenderer>>,
    #[cfg(feature = "browser")]
    browser_patterns: Vec<regex::Regex>,
    extractor: UrlExtractor,
    #[allow(dead_code)]
    semaphore: Arc<Semaphore>,
    concurrency: usize,
    metrics: Arc<WorkerMetrics>,
    shutdown: Arc<AtomicBool>,
    worker_id: String,
    link_graph: Option<Arc<LinkGraph>>,
    link_graph_interval: u64,
    incremental_crawl: bool,
    publish_links: bool,
    /// Tracks domains we've already discovered sitemaps for
    discovered_sitemap_domains: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    max_sitemap_urls: usize,
}

impl CrawlerWorker {
    /// Create a new crawler worker from CLI args (uses Kafka).
    async fn new(args: &Args) -> anyhow::Result<Self> {
        let worker_id = args
            .worker_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(worker_id = %worker_id, "Initializing crawler worker");

        // Create Kafka consumer
        let kafka_consumer = ConsumerBuilder::new(&args.brokers, &args.group_id)
            .client_id(format!("scrapix-crawler-{}", worker_id))
            .auto_offset_reset("earliest")
            .build()?;

        // Subscribe to processing topic (URLs ready to crawl after dedup/politeness)
        kafka_consumer.subscribe(&[topic_names::URL_PROCESSING])?;
        info!(
            topic = topic_names::URL_PROCESSING,
            "Subscribed to processing topic"
        );

        // Create Kafka producer
        let kafka_producer = ProducerBuilder::new(&args.brokers)
            .client_id(format!("scrapix-crawler-{}-producer", worker_id))
            .compression("lz4")
            .build()?;

        let consumer = AnyConsumer::from(kafka_consumer);
        let producer = AnyProducer::from(kafka_producer);

        Self::build(args, worker_id, consumer, producer).await
    }

    /// Create a new crawler worker using pre-built `AnyProducer`/`AnyConsumer` (for `scrapix all`).
    pub async fn with_bus(
        args: &Args,
        producer: AnyProducer,
        consumer: AnyConsumer,
    ) -> anyhow::Result<Self> {
        let worker_id = args
            .worker_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(worker_id = %worker_id, "Initializing crawler worker (in-process bus)");

        consumer.subscribe(&[topic_names::URL_PROCESSING])?;
        info!(
            topic = topic_names::URL_PROCESSING,
            "Subscribed to processing topic"
        );

        Self::build(args, worker_id, consumer, producer).await
    }

    /// Shared construction logic (everything except bus creation and subscription).
    async fn build(
        args: &Args,
        worker_id: String,
        consumer: AnyConsumer,
        producer: AnyProducer,
    ) -> anyhow::Result<Self> {
        // Create robots.txt cache configuration
        let robots_config = RobotsConfig {
            user_agent: args.user_agent.clone(),
            cache_ttl: Duration::from_secs(3600), // 1 hour
            fetch_timeout: Duration::from_secs(10),
            respect_robots: args.respect_robots,
            default_crawl_delay_ms: None,
        };

        // Create simple in-memory robots cache for the HTTP fetcher
        let fetcher_robots_cache = Arc::new(RobotsCache::new(robots_config.clone())?);

        // Create RocksDB storage for persistent state (sitemap discovery, robots persistence)
        info!(path = %args.rocksdb_path, "Initializing RocksDB storage");
        let rocks_config = RocksConfig {
            path: args.rocksdb_path.clone(),
            column_families: vec!["default".to_string(), "robots_cache".to_string()],
            ..Default::default()
        };
        let rocks_storage = RocksStorage::new(rocks_config)?;
        let rocks_adapter = Arc::new(RocksStorageAdapter::new(rocks_storage));

        // Create RocksDB-backed persistence for persistent robots cache (used for sitemap discovery)
        let robots_persistence: Arc<dyn RobotsPersistence> =
            Arc::new(RocksRobotsPersistenceAdapter::new(rocks_adapter));
        let persistent_robots_cache = Arc::new(PersistentRobotsCache::new(
            robots_config,
            robots_persistence,
        )?);

        // Warm up persistent robots cache from storage
        match persistent_robots_cache.warm_memory_cache() {
            Ok(loaded) => {
                if loaded > 0 {
                    info!(
                        count = loaded,
                        "Loaded robots.txt entries from persistent cache"
                    );
                }
            }
            Err(e) => warn!(error = %e, "Failed to warm robots cache"),
        }

        // Create sitemap parser if enabled
        let sitemap_parser = if args.sitemap_discovery {
            info!("Sitemap discovery enabled");
            Some(SitemapParser::with_defaults())
        } else {
            None
        };

        // Create HTTP fetcher with optional DNS caching
        let mut fetcher_builder = HttpFetcherBuilder::new()
            .user_agent(&args.user_agent)
            .timeout(Duration::from_secs(args.timeout))
            .max_retries(args.max_retries)
            .max_body_size(args.max_body_size_mb * 1024 * 1024);

        if args.dns_cache {
            fetcher_builder =
                fetcher_builder.dns_cache_ttl(Duration::from_secs(args.dns_cache_ttl));
            info!(ttl_secs = args.dns_cache_ttl, "DNS caching enabled");
        }

        let fetcher = fetcher_builder.build(fetcher_robots_cache)?;

        // Create URL extractor
        let extractor_config = ExtractorConfig {
            patterns: None,
            max_depth: args.max_depth,
            follow_external: args.follow_external,
            follow_subdomains: true,
            extract_from_data_attrs: false,
            allowed_domains: vec![],
        };
        let extractor = UrlExtractor::new(extractor_config);

        // Create link graph if enabled
        let link_graph = if args.link_graph {
            info!("Link graph tracking enabled");
            Some(Arc::new(LinkGraph::with_defaults()))
        } else {
            None
        };

        // Create browser renderer if enabled
        #[cfg(feature = "browser")]
        let (browser_renderer, browser_patterns) = if args.browser_render {
            // Parse browser render patterns
            let patterns: Vec<regex::Regex> = args
                .browser_render_patterns
                .as_ref()
                .map(|p| {
                    p.split(',')
                        .filter_map(|pattern| {
                            let pattern = pattern.trim();
                            if pattern.is_empty() {
                                return None;
                            }
                            match regex::Regex::new(pattern) {
                                Ok(r) => Some(r),
                                Err(e) => {
                                    warn!(
                                        pattern = pattern,
                                        error = %e,
                                        "Failed to compile browser render pattern"
                                    );
                                    None
                                }
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            info!(
                patterns_count = patterns.len(),
                "Browser render patterns compiled"
            );

            // Create CDP renderer
            let mut cdp_builder = CdpRendererBuilder::new()
                .timeout(Duration::from_secs(args.browser_timeout))
                .max_concurrent_pages(args.browser_concurrency)
                .headless(args.browser_headless);

            if let Some(ref chrome_path) = args.chrome_path {
                cdp_builder = cdp_builder.executable_path(chrome_path);
            }

            match cdp_builder.build().await {
                Ok(renderer) => {
                    info!(
                        headless = args.browser_headless,
                        concurrency = args.browser_concurrency,
                        timeout_secs = args.browser_timeout,
                        "Browser renderer initialized"
                    );
                    (Some(Arc::new(renderer)), patterns)
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Failed to initialize browser renderer, falling back to HTTP only"
                    );
                    (None, Vec::new())
                }
            }
        } else {
            (None, Vec::new())
        };

        #[cfg(not(feature = "browser"))]
        if args.browser_render {
            warn!("Browser rendering requested but 'browser' feature is not enabled. Compile with --features browser");
        }

        // Create concurrency limiter
        let semaphore = Arc::new(Semaphore::new(args.concurrency));

        Ok(Self {
            consumer,
            producer,
            fetcher,
            robots_cache: persistent_robots_cache,
            sitemap_parser,
            #[cfg(feature = "browser")]
            browser_renderer,
            #[cfg(feature = "browser")]
            browser_patterns,
            extractor,
            semaphore,
            concurrency: args.concurrency,
            metrics: Arc::new(WorkerMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            worker_id,
            link_graph,
            link_graph_interval: args.link_graph_interval,
            incremental_crawl: args.incremental_crawl,
            publish_links: args.publish_links,
            discovered_sitemap_domains: Arc::new(parking_lot::RwLock::new(HashSet::new())),
            max_sitemap_urls: args.max_sitemap_urls,
        })
    }

    /// Run the crawler worker
    async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        info!(worker_id = %self.worker_id, "Starting crawler worker main loop");

        // Start metrics reporter
        let metrics = self.metrics.clone();
        let shutdown = self.shutdown.clone();
        let link_graph = self.link_graph.clone();
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            while !shutdown.load(Ordering::Relaxed) {
                interval.tick().await;
                let snapshot = metrics.snapshot();

                // Log link graph stats if enabled
                let (link_graph_pages, link_graph_links) = if let Some(ref graph) = link_graph {
                    let stats = graph.stats();
                    (stats.page_count, stats.link_count)
                } else {
                    (0, 0)
                };

                info!(
                    processed = snapshot.urls_processed,
                    succeeded = snapshot.urls_succeeded,
                    failed = snapshot.urls_failed,
                    not_modified = snapshot.urls_not_modified,
                    discovered = snapshot.urls_discovered,
                    bytes_mb = snapshot.bytes_downloaded / (1024 * 1024),
                    active = snapshot.active_fetches,
                    http_fetches = snapshot.http_fetches,
                    browser_renders = snapshot.browser_renders,
                    dns_hits = snapshot.dns_cache_hits,
                    dns_misses = snapshot.dns_cache_misses,
                    link_graph_pages = link_graph_pages,
                    link_graph_links = link_graph_links,
                    "Worker metrics"
                );
            }
        });

        // Process messages using concurrent processing to maintain heartbeats
        let result = self.clone().process_messages().await;

        // Cleanup
        self.shutdown.store(true, Ordering::Relaxed);
        metrics_handle.abort();

        result
    }

    /// Process messages from the frontier queue
    /// Uses concurrent processing to keep the consumer polling and maintain heartbeats
    async fn process_messages(self: Arc<Self>) -> anyhow::Result<()> {
        let concurrency = self.concurrency;
        let shutdown = self.shutdown.clone();
        let worker = self.clone();

        info!(
            concurrency = concurrency,
            worker_id = %self.worker_id,
            "Starting message processing loop"
        );

        // Use concurrent processing to maintain heartbeats while handlers run
        self.consumer
            .process_concurrent::<UrlMessage, _, _>(
                move |msg, metadata| {
                    let worker = worker.clone();
                    async move {
                        info!(
                            url = %msg.url.url,
                            job_id = %msg.job_id,
                            partition = metadata.partition,
                            offset = metadata.offset,
                            "Received URL from frontier"
                        );

                        worker.metrics.fetch_started();
                        let result = worker.process_url(&msg).await;
                        worker.metrics.fetch_completed();

                        if let Err(ref e) = result {
                            warn!(
                                url = %msg.url.url,
                                job_id = %msg.job_id,
                                error = %e,
                                "Failed to process URL"
                            );
                        }

                        result
                    }
                },
                concurrency,
                shutdown,
            )
            .await?;

        Ok(())
    }

    /// Check if a URL should be rendered with a browser
    #[cfg(feature = "browser")]
    fn should_use_browser(&self, url: &scrapix_core::CrawlUrl) -> bool {
        // Check if browser renderer is available
        if self.browser_renderer.is_none() {
            return false;
        }

        // Check if URL explicitly requires JS
        if url.requires_js {
            return true;
        }

        // Check against configured patterns
        for pattern in &self.browser_patterns {
            if pattern.is_match(&url.url) {
                return true;
            }
        }

        false
    }

    /// Fetch a page using the browser renderer
    #[cfg(feature = "browser")]
    async fn fetch_with_browser(
        &self,
        url: &scrapix_core::CrawlUrl,
    ) -> scrapix_core::Result<scrapix_core::RawPage> {
        let renderer = self
            .browser_renderer
            .as_ref()
            .ok_or_else(|| ScrapixError::Crawl("Browser renderer not available".into()))?;

        renderer.fetch(url).await
    }

    /// Discover and publish sitemap URLs for a domain (if not already done)
    #[allow(clippy::too_many_arguments)]
    async fn maybe_discover_sitemaps(
        &self,
        domain: &str,
        job_id: &str,
        index_uid: &str,
        url_patterns: Option<UrlPatterns>,
        meilisearch_url: Option<String>,
        meilisearch_api_key: Option<String>,
        features: Option<FeaturesConfig>,
    ) -> scrapix_core::Result<usize> {
        // Check if we've already discovered sitemaps for this domain
        {
            let domains = self.discovered_sitemap_domains.read();
            if domains.contains(domain) {
                return Ok(0);
            }
        }

        // Mark as discovered (even before we try, to avoid duplicate work)
        {
            let mut domains = self.discovered_sitemap_domains.write();
            domains.insert(domain.to_string());
        }

        let sitemap_parser = match &self.sitemap_parser {
            Some(parser) => parser,
            None => return Ok(0),
        };

        // Get sitemap URLs from robots.txt (via persistent cache)
        let sitemap_urls = match self.robots_cache.get_sitemaps(domain).await {
            Ok(urls) => urls,
            Err(e) => {
                debug!(domain, error = %e, "Failed to get sitemaps from robots.txt");
                return Ok(0);
            }
        };

        if sitemap_urls.is_empty() {
            debug!(domain, "No sitemaps found in robots.txt");
            return Ok(0);
        }

        info!(
            domain,
            sitemap_count = sitemap_urls.len(),
            "Found sitemaps in robots.txt"
        );

        // Parse each sitemap and collect URLs
        let mut discovered_count = 0;
        for sitemap_url in sitemap_urls {
            // fetch_and_parse recursively expands sitemap indexes and returns all URLs
            let urls = match sitemap_parser.fetch_and_parse(&sitemap_url).await {
                Ok(urls) => urls,
                Err(e) => {
                    debug!(sitemap_url, error = %e, "Failed to fetch/parse sitemap");
                    continue;
                }
            };

            debug!(sitemap_url, url_count = urls.len(), "Parsed sitemap");

            let urls_to_publish = urls
                .into_iter()
                .take(self.max_sitemap_urls - discovered_count);

            for sitemap_entry in urls_to_publish {
                // Filter sitemap URLs using allowed_domains whitelist first (strictest check)
                if let Some(ref patterns) = url_patterns {
                    if !patterns.allowed_domains.is_empty() {
                        // Extract domain from sitemap URL
                        if let Ok(parsed_url) = url::Url::parse(&sitemap_entry.loc) {
                            if let Some(url_domain) = parsed_url.host_str() {
                                let domain_allowed = patterns
                                    .allowed_domains
                                    .iter()
                                    .any(|d| d.eq_ignore_ascii_case(url_domain));
                                if !domain_allowed {
                                    continue; // Skip URLs from non-whitelisted domains
                                }
                            }
                        }
                    }
                }

                // Filter sitemap URLs using glob patterns if available
                if let Some(ref patterns) = url_patterns {
                    let matches_include = patterns.include.is_empty()
                        || patterns.include.iter().any(|p| {
                            if p.contains("**") {
                                let parts: Vec<&str> = p.split("**").collect();
                                parts.len() == 2
                                    && sitemap_entry.loc.starts_with(parts[0])
                                    && (parts[1].is_empty()
                                        || sitemap_entry.loc.ends_with(parts[1]))
                            } else {
                                sitemap_entry.loc == *p
                            }
                        });
                    let matches_exclude = patterns.exclude.iter().any(|p| {
                        if p.contains("**") {
                            let parts: Vec<&str> = p.split("**").collect();
                            parts.len() == 2
                                && sitemap_entry.loc.starts_with(parts[0])
                                && (parts[1].is_empty() || sitemap_entry.loc.ends_with(parts[1]))
                        } else {
                            sitemap_entry.loc == *p
                        }
                    });
                    if !matches_include || matches_exclude {
                        continue; // Skip URLs that don't match patterns
                    }
                }

                // Create a CrawlUrl from the sitemap entry
                let mut crawl_url = CrawlUrl::seed(&sitemap_entry.loc);
                // Mark as discovered from sitemap via parent_url
                crawl_url.parent_url = Some(format!("sitemap:{}", sitemap_url));

                // Boost priority based on sitemap priority
                if let Some(priority) = sitemap_entry.priority {
                    crawl_url.priority = (priority * 100.0) as i32;
                }

                // Include patterns and meilisearch config when publishing sitemap URLs
                let url_msg = match &url_patterns {
                    Some(patterns) => {
                        UrlMessage::with_patterns(crawl_url, job_id, index_uid, patterns.clone())
                    }
                    None => UrlMessage::new(crawl_url, job_id, index_uid),
                }
                .with_meilisearch(meilisearch_url.clone(), meilisearch_api_key.clone())
                .with_features(features.clone());

                if let Err(e) = self
                    .producer
                    .send(
                        topic_names::URL_FRONTIER,
                        Some(&url_msg.partition_key()),
                        &url_msg,
                    )
                    .await
                {
                    warn!(
                        url = sitemap_entry.loc,
                        error = %e,
                        "Failed to publish sitemap URL"
                    );
                } else {
                    discovered_count += 1;
                }

                if discovered_count >= self.max_sitemap_urls {
                    info!(
                        domain,
                        max = self.max_sitemap_urls,
                        "Reached max sitemap URLs limit"
                    );
                    break;
                }
            }
        }

        if discovered_count > 0 {
            self.metrics
                .record_sitemap_discovery(discovered_count as u64);
            info!(
                domain,
                discovered_count, "Published sitemap URLs to frontier"
            );
        }

        Ok(discovered_count)
    }

    /// Process a single URL
    async fn process_url(&self, msg: &UrlMessage) -> scrapix_core::Result<()> {
        let start = Instant::now();
        let url = &msg.url;

        // Try to discover sitemaps for this domain (only runs once per domain)
        if let Ok(parsed_url) = url::Url::parse(&url.url) {
            if let Some(domain) = parsed_url.host_str() {
                if let Err(e) = self
                    .maybe_discover_sitemaps(
                        domain,
                        &msg.job_id,
                        &msg.index_uid,
                        msg.url_patterns.clone(),
                        msg.meilisearch_url.clone(),
                        msg.meilisearch_api_key.clone(),
                        msg.features.clone(),
                    )
                    .await
                {
                    debug!(domain, error = %e, "Sitemap discovery failed");
                }
            }
        }

        // Determine if we should use browser rendering
        #[cfg(feature = "browser")]
        let use_browser = self.should_use_browser(url);
        #[cfg(not(feature = "browser"))]
        let use_browser = false;

        // Fetch the page (either with HTTP or browser)
        let page = if use_browser {
            #[cfg(feature = "browser")]
            {
                debug!(url = %url.url, "Using browser rendering");
                self.metrics.record_browser_render();
                match self.fetch_with_browser(url).await {
                    Ok(page) => page,
                    Err(e) => {
                        self.metrics.record_failure();
                        let event = CrawlEvent::page_failed(
                            &msg.job_id,
                            &url.url,
                            e.to_string(),
                            url.retry_count,
                        );
                        self.publish_event(&msg.job_id, &event).await?;
                        return Err(e);
                    }
                }
            }
            #[cfg(not(feature = "browser"))]
            {
                unreachable!("Browser feature not enabled")
            }
        } else {
            // Build conditional headers for incremental crawling
            let conditional_headers = if self.incremental_crawl {
                let mut headers = ConditionalRequestHeaders::new();
                if let Some(ref etag) = url.etag {
                    headers = headers.with_etag(etag);
                }
                if let Some(ref last_modified) = url.last_modified {
                    headers = headers.with_last_modified(last_modified);
                }
                headers
            } else {
                ConditionalRequestHeaders::new()
            };

            self.metrics.record_http_fetch();

            // Fetch the page with conditional headers
            match self
                .fetcher
                .fetch_conditional(url, &conditional_headers)
                .await
            {
                Ok(result) => match result {
                    scrapix_crawler::FetchResult::Fetched(page) => page,
                    scrapix_crawler::FetchResult::NotModified {
                        url: not_modified_url,
                        fetch_duration_ms: _,
                    } => {
                        // Page hasn't changed since last crawl
                        self.metrics.record_not_modified();
                        debug!(
                            url = %url.url,
                            "Page not modified (304), skipping processing"
                        );

                        // Publish skip event so the API can track job progress
                        let event = CrawlEvent::PageSkipped {
                            job_id: msg.job_id.clone(),
                            url: not_modified_url,
                            reason: "304 Not Modified".to_string(),
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };
                        self.publish_event(&msg.job_id, &event).await?;

                        return Ok(());
                    }
                },
                Err(e) => {
                    self.metrics.record_failure();

                    // Send failure event
                    let event = CrawlEvent::page_failed(
                        &msg.job_id,
                        &url.url,
                        e.to_string(),
                        url.retry_count,
                    );
                    self.publish_event(&msg.job_id, &event).await?;

                    return Err(e);
                }
            }
        };

        let fetch_duration = start.elapsed();
        let page_size = page.html.len() as u64;

        // Update DNS cache stats
        if let Some(dns_stats) = self.fetcher.dns_cache_stats() {
            self.metrics
                .record_dns_stats(dns_stats.hits, dns_stats.misses);
        }

        self.metrics.record_success(page_size);

        info!(
            url = %url.url,
            status = page.status,
            size_kb = page_size / 1024,
            duration_ms = fetch_duration.as_millis(),
            js_rendered = page.js_rendered,
            "Page fetched successfully"
        );

        // Extract URLs from the page using patterns from the message if available
        let discovered_urls = if let Some(ref patterns) = msg.url_patterns {
            // Create extractor with patterns from job config
            // If allowed_domains is set, use strict domain filtering
            let extractor_config = ExtractorConfig {
                patterns: Some(patterns.clone()),
                max_depth: self.concurrency as u32, // Use existing max_depth
                follow_external: false,
                follow_subdomains: patterns.allowed_domains.is_empty(), // Disable if whitelist is set
                extract_from_data_attrs: false,
                allowed_domains: patterns.allowed_domains.clone(),
            };
            let extractor = UrlExtractor::new(extractor_config);
            extractor.extract(&page, url.depth)
        } else {
            // Use default extractor without patterns
            self.extractor.extract(&page, url.depth)
        };
        let discovered_count = discovered_urls.len();

        self.metrics.record_discovered(discovered_count as u64);

        // Track links in link graph if enabled
        let target_urls: Vec<String> = discovered_urls.iter().map(|u| u.url.clone()).collect();
        if let Some(ref graph) = self.link_graph {
            let target_refs: Vec<&str> = target_urls.iter().map(|s| s.as_str()).collect();
            graph.record_links(&url.url, target_refs);

            // Periodically compute scores
            let processed = self.metrics.urls_processed.load(Ordering::Relaxed);
            if processed > 0 && processed % self.link_graph_interval == 0 {
                graph.compute_scores_if_dirty();
                debug!(processed = processed, "Recomputed link graph scores");
            }
        }

        // Publish links to frontier service for centralized PageRank if enabled
        if self.publish_links && !target_urls.is_empty() {
            let links_msg = LinksMessage::new(&url.url, target_urls.clone(), &msg.job_id);
            if let Err(e) = self
                .producer
                .send(topic_names::LINKS, Some(&msg.job_id), &links_msg)
                .await
            {
                debug!(error = %e, "Failed to publish links to frontier");
            }
        }

        debug!(
            url = %url.url,
            count = discovered_count,
            "Extracted URLs from page"
        );

        // Extract ETag and Last-Modified from response headers for incremental crawling
        let etag = page.headers.get("etag").cloned();
        let last_modified = page.headers.get("last-modified").cloned();

        // Publish raw page to content processing topic
        let raw_page_msg = RawPageMessage {
            url: page.url.clone(),
            final_url: page.final_url.clone(),
            status: page.status,
            html: page.html,
            content_type: page.content_type,
            content_length: page_size,
            js_rendered: page.js_rendered,
            fetched_at: page.fetched_at.timestamp_millis(),
            fetch_duration_ms: page.fetch_duration_ms,
            job_id: msg.job_id.clone(),
            index_uid: msg.index_uid.clone(),
            account_id: msg.account_id.clone(),
            message_id: uuid::Uuid::new_v4().to_string(),
            etag,
            last_modified,
            meilisearch_url: msg.meilisearch_url.clone(),
            meilisearch_api_key: msg.meilisearch_api_key.clone(),
            features: msg.features.clone(),
        };

        self.producer
            .send(topic_names::PAGES_RAW, Some(&msg.job_id), &raw_page_msg)
            .await?;

        debug!(
            url = %page.url,
            topic = topic_names::PAGES_RAW,
            "Published raw page to content topic"
        );

        // Publish discovered URLs back to frontier with link graph boost
        for mut discovered_url in discovered_urls {
            // Apply link graph priority boost if enabled
            if let Some(ref graph) = self.link_graph {
                let boost = graph.get_priority_boost(&discovered_url.url);
                discovered_url.priority += boost;
            }

            // Propagate URL patterns and account_id from parent message so child URLs are filtered correctly
            let mut url_msg = if let Some(ref patterns) = msg.url_patterns {
                UrlMessage::with_patterns(
                    discovered_url,
                    &msg.job_id,
                    &msg.index_uid,
                    patterns.clone(),
                )
            } else {
                UrlMessage::new(discovered_url, &msg.job_id, &msg.index_uid)
            };
            // Propagate account_id for billing attribution
            url_msg.account_id = msg.account_id.clone();
            // Propagate per-job Meilisearch config
            url_msg.meilisearch_url = msg.meilisearch_url.clone();
            url_msg.meilisearch_api_key = msg.meilisearch_api_key.clone();
            // Propagate per-job feature config
            url_msg.features = msg.features.clone();

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

        // Send success event with billing data
        let event = CrawlEvent::page_crawled_with_billing(
            &msg.job_id,
            msg.account_id.clone(),
            &url.url,
            page.status,
            page_size,
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

/// Run the crawler worker with the given arguments.
pub async fn run(args: Args) -> anyhow::Result<()> {
    info!(
        concurrency = args.concurrency,
        brokers = %args.brokers,
        group_id = %args.group_id,
        dns_cache = args.dns_cache,
        link_graph = args.link_graph,
        incremental_crawl = args.incremental_crawl,
        browser_render = args.browser_render,
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

    // Clone for metrics access after run completes
    let worker_for_metrics = worker.clone();

    // Run the worker (consumes the Arc)
    let result = worker.run().await;

    // Cleanup
    shutdown_handle.abort();

    // Print final metrics
    let metrics = worker_for_metrics.metrics.snapshot();
    info!(
        processed = metrics.urls_processed,
        succeeded = metrics.urls_succeeded,
        failed = metrics.urls_failed,
        not_modified = metrics.urls_not_modified,
        discovered = metrics.urls_discovered,
        bytes_mb = metrics.bytes_downloaded / (1024 * 1024),
        http_fetches = metrics.http_fetches,
        browser_renders = metrics.browser_renders,
        dns_hits = metrics.dns_cache_hits,
        dns_misses = metrics.dns_cache_misses,
        "Final worker metrics"
    );

    // Print link graph stats if enabled
    if let Some(ref graph) = worker_for_metrics.link_graph {
        let stats = graph.stats();
        info!(
            pages = stats.page_count,
            links = stats.link_count,
            avg_score = format!("{:.4}", stats.avg_score),
            "Final link graph stats"
        );
    }

    result
}

/// Run the crawler worker using pre-built message bus trait objects.
///
/// Used by `scrapix all` to run the crawler in-process alongside other services.
pub async fn run_with_bus(
    args: Args,
    producer: AnyProducer,
    consumer: AnyConsumer,
) -> anyhow::Result<()> {
    info!(
        concurrency = args.concurrency,
        "Starting Scrapix crawler worker (in-process bus)"
    );

    let worker = Arc::new(CrawlerWorker::with_bus(&args, producer, consumer).await?);
    let worker_for_metrics = worker.clone();

    let result = worker.run().await;

    let metrics = worker_for_metrics.metrics.snapshot();
    info!(
        processed = metrics.urls_processed,
        succeeded = metrics.urls_succeeded,
        failed = metrics.urls_failed,
        "Final crawler worker metrics (in-process)"
    );

    result
}
