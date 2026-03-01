//! Scrapix Frontier Service
//!
//! URL frontier management with deduplication and politeness scheduling.
//!
//! ## Responsibilities
//!
//! 1. Consume raw URLs from the frontier topic
//! 2. Deduplicate URLs using Bloom filters (per-job)
//! 3. Apply priority scoring based on depth and explicit priority
//! 4. Enforce politeness delays per domain
//! 5. Publish ready-to-crawl URLs to the processing topic
//!
//! ## Architecture
//!
//! ```text
//! URL_FRONTIER → [Dedup] → [Priority Queue] → [Politeness] → URL_PROCESSING
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use scrapix_core::{CrawlUrl, ScrapixError, UrlPatterns};
use scrapix_frontier::{
    extract_domain, CrawlRecord, DedupConfig, LinkGraph, LinkGraphConfig, PolitenessConfig,
    PolitenessScheduler, PriorityConfig, PriorityQueue, RecrawlConfig, RecrawlDecision,
    RecrawlScheduler, UrlDedup, UrlHistory, UrlHistoryConfig,
};
use scrapix_queue::{
    topic_names, AnyConsumer, AnyProducer, ConsumerBuilder, CrawlHistoryMessage, LinksMessage,
    ProducerBuilder, UrlMessage,
};

/// Scrapix Frontier Service
#[derive(Parser, Debug)]
#[command(name = "scrapix-frontier-service")]
#[command(
    version,
    about = "URL frontier management service with deduplication and politeness"
)]
pub struct Args {
    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    pub brokers: String,

    /// Consumer group ID
    #[arg(
        short,
        long,
        env = "KAFKA_GROUP_ID",
        default_value = "scrapix-frontier"
    )]
    pub group_id: String,

    /// Bloom filter expected capacity per job
    #[arg(long, env = "BLOOM_CAPACITY", default_value = "10000000")]
    pub bloom_capacity: usize,

    /// Bloom filter false positive rate
    #[arg(long, env = "BLOOM_FP_RATE", default_value = "0.01")]
    pub bloom_fp_rate: f64,

    /// Default delay between requests to the same domain (ms)
    #[arg(long, env = "DOMAIN_DELAY_MS", default_value = "50")]
    pub domain_delay_ms: u64,

    /// Maximum concurrent requests per domain
    #[arg(long, env = "CONCURRENT_PER_DOMAIN", default_value = "50")]
    pub concurrent_per_domain: usize,

    /// URL dispatch batch size
    #[arg(long, env = "DISPATCH_BATCH_SIZE", default_value = "2000")]
    pub dispatch_batch_size: usize,

    /// Dispatch interval (ms)
    #[arg(long, env = "DISPATCH_INTERVAL_MS", default_value = "20")]
    pub dispatch_interval_ms: u64,

    /// Maximum pending URLs per job
    #[arg(long, env = "MAX_PENDING_PER_JOB", default_value = "1000000")]
    pub max_pending_per_job: usize,

    /// Service instance ID
    #[arg(long, env = "INSTANCE_ID")]
    pub instance_id: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    // === LINK GRAPH OPTIONS ===
    /// Enable PageRank-based prioritization
    #[arg(long, env = "ENABLE_LINKGRAPH", default_value = "false")]
    pub enable_linkgraph: bool,

    /// PageRank damping factor (0.0-1.0)
    #[arg(long, env = "LINKGRAPH_DAMPING", default_value = "0.85")]
    pub linkgraph_damping: f64,

    /// Maximum priority boost from PageRank
    #[arg(long, env = "LINKGRAPH_MAX_BOOST", default_value = "50")]
    pub linkgraph_max_boost: i32,

    /// Maximum pages to track in link graph (0 = unlimited)
    #[arg(long, env = "LINKGRAPH_MAX_PAGES", default_value = "10000000")]
    pub linkgraph_max_pages: usize,

    /// PageRank computation interval in seconds
    #[arg(long, env = "LINKGRAPH_COMPUTE_INTERVAL", default_value = "300")]
    pub linkgraph_compute_interval: u64,

    // === RECRAWL OPTIONS ===
    /// Enable incremental recrawl scheduling
    #[arg(long, env = "ENABLE_RECRAWL", default_value = "false")]
    pub enable_recrawl: bool,

    /// Minimum age before allowing recrawl (seconds)
    #[arg(long, env = "RECRAWL_MIN_AGE", default_value = "3600")]
    pub recrawl_min_age: u64,

    /// Maximum age before forcing recrawl (seconds)
    #[arg(long, env = "RECRAWL_MAX_AGE", default_value = "604800")]
    pub recrawl_max_age: u64,

    /// Maximum URLs to track in history (0 = unlimited)
    #[arg(long, env = "RECRAWL_MAX_URLS", default_value = "10000000")]
    pub recrawl_max_urls: usize,
}

/// Per-job frontier state
struct JobFrontier {
    job_id: String,
    index_uid: String,
    dedup: UrlDedup,
    queue: PriorityQueue,
    #[allow(dead_code)]
    created_at: chrono::DateTime<chrono::Utc>,
    urls_received: AtomicU64,
    urls_deduplicated: AtomicU64,
    urls_dispatched: AtomicU64,
    url_patterns: Option<UrlPatterns>,
    meilisearch_url: Option<String>,
    meilisearch_api_key: Option<String>,
}

impl JobFrontier {
    fn new(
        job_id: &str,
        index_uid: &str,
        dedup_config: &DedupConfig,
        priority_config: &PriorityConfig,
        url_patterns: Option<UrlPatterns>,
        meilisearch_url: Option<String>,
        meilisearch_api_key: Option<String>,
    ) -> Self {
        Self {
            job_id: job_id.to_string(),
            index_uid: index_uid.to_string(),
            dedup: UrlDedup::new(dedup_config.clone()),
            queue: PriorityQueue::new(priority_config.clone()),
            created_at: chrono::Utc::now(),
            urls_received: AtomicU64::new(0),
            urls_deduplicated: AtomicU64::new(0),
            urls_dispatched: AtomicU64::new(0),
            url_patterns,
            meilisearch_url,
            meilisearch_api_key,
        }
    }

    fn try_add(&self, url: CrawlUrl) -> bool {
        self.urls_received.fetch_add(1, Ordering::Relaxed);

        if self.dedup.check_and_mark(&url.url) {
            self.urls_deduplicated.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            self.queue.push(url);
            true
        }
    }

    fn pop_ready(&self, count: usize) -> Vec<CrawlUrl> {
        let urls = self.queue.pop_many(count);
        self.urls_dispatched
            .fetch_add(urls.len() as u64, Ordering::Relaxed);
        urls
    }

    #[allow(dead_code)]
    fn pending_count(&self) -> usize {
        self.queue.len()
    }

    fn stats(&self) -> JobStats {
        JobStats {
            job_id: self.job_id.clone(),
            urls_received: self.urls_received.load(Ordering::Relaxed),
            urls_deduplicated: self.urls_deduplicated.load(Ordering::Relaxed),
            urls_dispatched: self.urls_dispatched.load(Ordering::Relaxed),
            urls_pending: self.queue.len() as u64,
            dedup_stats: self.dedup.stats(),
        }
    }
}

#[derive(Debug)]
struct JobStats {
    job_id: String,
    urls_received: u64,
    urls_deduplicated: u64,
    urls_dispatched: u64,
    urls_pending: u64,
    dedup_stats: scrapix_frontier::DedupStats,
}

#[derive(Debug, Default)]
struct ServiceMetrics {
    messages_consumed: AtomicU64,
    urls_received: AtomicU64,
    urls_new: AtomicU64,
    urls_duplicate: AtomicU64,
    urls_dispatched: AtomicU64,
    urls_delayed: AtomicU64,
    urls_recrawl_skipped: AtomicU64,
    active_jobs: AtomicU64,
    active_domains: AtomicU64,
    links_recorded: AtomicU64,
    history_updates: AtomicU64,
}

impl ServiceMetrics {
    fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            messages_consumed: self.messages_consumed.load(Ordering::Relaxed),
            urls_received: self.urls_received.load(Ordering::Relaxed),
            urls_new: self.urls_new.load(Ordering::Relaxed),
            urls_duplicate: self.urls_duplicate.load(Ordering::Relaxed),
            urls_dispatched: self.urls_dispatched.load(Ordering::Relaxed),
            urls_delayed: self.urls_delayed.load(Ordering::Relaxed),
            urls_recrawl_skipped: self.urls_recrawl_skipped.load(Ordering::Relaxed),
            active_jobs: self.active_jobs.load(Ordering::Relaxed),
            active_domains: self.active_domains.load(Ordering::Relaxed),
            links_recorded: self.links_recorded.load(Ordering::Relaxed),
            history_updates: self.history_updates.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct MetricsSnapshot {
    messages_consumed: u64,
    urls_received: u64,
    urls_new: u64,
    urls_duplicate: u64,
    urls_dispatched: u64,
    urls_delayed: u64,
    urls_recrawl_skipped: u64,
    active_jobs: u64,
    active_domains: u64,
    links_recorded: u64,
    history_updates: u64,
}

struct ReadyUrl {
    url: CrawlUrl,
    job_id: String,
    index_uid: String,
    domain: String,
    url_patterns: Option<UrlPatterns>,
    meilisearch_url: Option<String>,
    meilisearch_api_key: Option<String>,
}

struct FrontierService {
    consumer: Arc<AnyConsumer>,
    links_consumer: Option<Arc<AnyConsumer>>,
    history_consumer: Option<Arc<AnyConsumer>>,
    producer: Arc<AnyProducer>,
    jobs: Arc<RwLock<HashMap<String, Arc<JobFrontier>>>>,
    politeness: Arc<PolitenessScheduler>,
    link_graph: Option<Arc<LinkGraph>>,
    recrawl_scheduler: Option<Arc<RecrawlScheduler>>,
    url_history: Option<Arc<UrlHistory>>,
    metrics: Arc<ServiceMetrics>,
    shutdown: Arc<AtomicBool>,
    instance_id: String,
    dedup_config: DedupConfig,
    priority_config: PriorityConfig,
    dispatch_batch_size: usize,
    dispatch_interval: Duration,
    linkgraph_compute_interval: Duration,
}

impl FrontierService {
    async fn new(args: &Args) -> anyhow::Result<Self> {
        let instance_id = args
            .instance_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(instance_id = %instance_id, "Initializing frontier service");

        let kafka_consumer = ConsumerBuilder::new(&args.brokers, &args.group_id)
            .client_id(format!("scrapix-frontier-{}", instance_id))
            .auto_offset_reset("earliest")
            .build()?;

        kafka_consumer.subscribe(&[topic_names::URL_FRONTIER])?;
        info!(
            topic = topic_names::URL_FRONTIER,
            "Subscribed to frontier topic"
        );

        let consumer: Arc<AnyConsumer> = Arc::new(AnyConsumer::from(kafka_consumer));

        let kafka_producer = ProducerBuilder::new(&args.brokers)
            .client_id(format!("scrapix-frontier-{}-producer", instance_id))
            .compression("lz4")
            .build()?;

        let producer: Arc<AnyProducer> = Arc::new(AnyProducer::from(kafka_producer));

        let politeness_config = PolitenessConfig {
            default_delay_ms: args.domain_delay_ms,
            min_delay_ms: 100,
            max_delay_ms: 30_000,
            respect_robots_delay: true,
            robots_delay_multiplier: 1.0,
            concurrent_per_domain: args.concurrent_per_domain,
        };
        let politeness = PolitenessScheduler::new(politeness_config);

        let dedup_config = DedupConfig {
            expected_items: args.bloom_capacity,
            false_positive_rate: args.bloom_fp_rate,
            use_partitioned: false,
            partition_count: 16,
        };

        let priority_config = PriorityConfig {
            max_size: args.max_pending_per_job,
            priority_weight: 100,
            depth_weight: -10,
            seed_bonus: 1000,
        };

        let (link_graph, links_consumer) = if args.enable_linkgraph {
            let config = LinkGraphConfig {
                damping_factor: args.linkgraph_damping,
                max_priority_boost: args.linkgraph_max_boost,
                max_pages: args.linkgraph_max_pages,
                ..Default::default()
            };
            let graph = Arc::new(LinkGraph::new(config));
            info!(
                damping = args.linkgraph_damping,
                max_boost = args.linkgraph_max_boost,
                max_pages = args.linkgraph_max_pages,
                "LinkGraph enabled for PageRank-based prioritization"
            );

            let kafka_links_consumer =
                ConsumerBuilder::new(&args.brokers, format!("{}-links", args.group_id))
                    .client_id(format!("scrapix-frontier-{}-links", instance_id))
                    .auto_offset_reset("earliest")
                    .build()?;
            kafka_links_consumer.subscribe(&[topic_names::LINKS])?;
            info!(topic = topic_names::LINKS, "Subscribed to links topic");

            let links_consumer: Arc<AnyConsumer> = Arc::new(AnyConsumer::from(kafka_links_consumer));

            (Some(graph), Some(links_consumer))
        } else {
            (None, None)
        };

        let (recrawl_scheduler, url_history, history_consumer) = if args.enable_recrawl {
            let history_config = UrlHistoryConfig {
                max_entries: args.recrawl_max_urls,
                min_recrawl_interval: Duration::from_secs(args.recrawl_min_age),
                max_recrawl_interval: Duration::from_secs(args.recrawl_max_age),
                ..Default::default()
            };
            let history = Arc::new(UrlHistory::new(history_config));

            let recrawl_config = RecrawlConfig {
                enabled: true,
                min_age: Duration::from_secs(args.recrawl_min_age),
                max_age: Duration::from_secs(args.recrawl_max_age),
                ..Default::default()
            };
            let scheduler = Arc::new(RecrawlScheduler::new(recrawl_config, history.clone()));
            info!(
                min_age_secs = args.recrawl_min_age,
                max_age_secs = args.recrawl_max_age,
                max_urls = args.recrawl_max_urls,
                "RecrawlScheduler enabled for incremental crawling"
            );

            let kafka_history_consumer =
                ConsumerBuilder::new(&args.brokers, format!("{}-history", args.group_id))
                    .client_id(format!("scrapix-frontier-{}-history", instance_id))
                    .auto_offset_reset("earliest")
                    .build()?;
            kafka_history_consumer.subscribe(&[topic_names::CRAWL_HISTORY])?;
            info!(
                topic = topic_names::CRAWL_HISTORY,
                "Subscribed to crawl history topic"
            );

            let history_consumer: Arc<AnyConsumer> =
                Arc::new(AnyConsumer::from(kafka_history_consumer));

            (Some(scheduler), Some(history), Some(history_consumer))
        } else {
            (None, None, None)
        };

        Ok(Self {
            consumer,
            links_consumer,
            history_consumer,
            producer,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            politeness: Arc::new(politeness),
            link_graph,
            recrawl_scheduler,
            url_history,
            metrics: Arc::new(ServiceMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            instance_id,
            dedup_config,
            priority_config,
            dispatch_batch_size: args.dispatch_batch_size,
            dispatch_interval: Duration::from_millis(args.dispatch_interval_ms),
            linkgraph_compute_interval: Duration::from_secs(args.linkgraph_compute_interval),
        })
    }

    /// Create a `FrontierService` from pre-built message bus trait objects.
    ///
    /// Used by `scrapix all` which constructs a shared in-process bus and passes
    /// the relevant producers/consumers to each service.
    pub async fn with_bus(
        args: &Args,
        producer: Arc<AnyProducer>,
        main_consumer: Arc<AnyConsumer>,
        links_consumer: Option<Arc<AnyConsumer>>,
        history_consumer: Option<Arc<AnyConsumer>>,
    ) -> anyhow::Result<Self> {
        let instance_id = args
            .instance_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(instance_id = %instance_id, "Initializing frontier service (pre-built bus)");

        let politeness_config = PolitenessConfig {
            default_delay_ms: args.domain_delay_ms,
            min_delay_ms: 100,
            max_delay_ms: 30_000,
            respect_robots_delay: true,
            robots_delay_multiplier: 1.0,
            concurrent_per_domain: args.concurrent_per_domain,
        };
        let politeness = PolitenessScheduler::new(politeness_config);

        let dedup_config = DedupConfig {
            expected_items: args.bloom_capacity,
            false_positive_rate: args.bloom_fp_rate,
            use_partitioned: false,
            partition_count: 16,
        };

        let priority_config = PriorityConfig {
            max_size: args.max_pending_per_job,
            priority_weight: 100,
            depth_weight: -10,
            seed_bonus: 1000,
        };

        let link_graph = if args.enable_linkgraph {
            let config = LinkGraphConfig {
                damping_factor: args.linkgraph_damping,
                max_priority_boost: args.linkgraph_max_boost,
                max_pages: args.linkgraph_max_pages,
                ..Default::default()
            };
            let graph = Arc::new(LinkGraph::new(config));
            info!(
                damping = args.linkgraph_damping,
                max_boost = args.linkgraph_max_boost,
                max_pages = args.linkgraph_max_pages,
                "LinkGraph enabled for PageRank-based prioritization"
            );
            Some(graph)
        } else {
            None
        };

        let (recrawl_scheduler, url_history) = if args.enable_recrawl {
            let history_config = UrlHistoryConfig {
                max_entries: args.recrawl_max_urls,
                min_recrawl_interval: Duration::from_secs(args.recrawl_min_age),
                max_recrawl_interval: Duration::from_secs(args.recrawl_max_age),
                ..Default::default()
            };
            let history = Arc::new(UrlHistory::new(history_config));

            let recrawl_config = RecrawlConfig {
                enabled: true,
                min_age: Duration::from_secs(args.recrawl_min_age),
                max_age: Duration::from_secs(args.recrawl_max_age),
                ..Default::default()
            };
            let scheduler = Arc::new(RecrawlScheduler::new(recrawl_config, history.clone()));
            info!(
                min_age_secs = args.recrawl_min_age,
                max_age_secs = args.recrawl_max_age,
                max_urls = args.recrawl_max_urls,
                "RecrawlScheduler enabled for incremental crawling"
            );
            (Some(scheduler), Some(history))
        } else {
            (None, None)
        };

        Ok(Self {
            consumer: main_consumer,
            links_consumer,
            history_consumer,
            producer,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            politeness: Arc::new(politeness),
            link_graph,
            recrawl_scheduler,
            url_history,
            metrics: Arc::new(ServiceMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            instance_id,
            dedup_config,
            priority_config,
            dispatch_batch_size: args.dispatch_batch_size,
            dispatch_interval: Duration::from_millis(args.dispatch_interval_ms),
            linkgraph_compute_interval: Duration::from_secs(args.linkgraph_compute_interval),
        })
    }

    fn get_or_create_job(
        &self,
        job_id: &str,
        index_uid: &str,
        url_patterns: Option<UrlPatterns>,
        meilisearch_url: Option<String>,
        meilisearch_api_key: Option<String>,
    ) -> Arc<JobFrontier> {
        {
            let jobs = self.jobs.read();
            if let Some(job) = jobs.get(job_id) {
                return job.clone();
            }
        }

        let mut jobs = self.jobs.write();
        jobs.entry(job_id.to_string())
            .or_insert_with(|| {
                info!(job_id = %job_id, index_uid = %index_uid, "Creating new job frontier");
                self.metrics.active_jobs.fetch_add(1, Ordering::Relaxed);
                Arc::new(JobFrontier::new(
                    job_id,
                    index_uid,
                    &self.dedup_config,
                    &self.priority_config,
                    url_patterns,
                    meilisearch_url,
                    meilisearch_api_key,
                ))
            })
            .clone()
    }

    async fn run(&self) -> anyhow::Result<()> {
        info!(instance_id = %self.instance_id, "Starting frontier service main loop");

        let metrics = self.metrics.clone();
        let link_graph = self.link_graph.clone();
        let url_history = self.url_history.clone();
        let shutdown = self.shutdown.clone();
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            while !shutdown.load(Ordering::Relaxed) {
                interval.tick().await;
                let snapshot = metrics.snapshot();
                info!(
                    consumed = snapshot.messages_consumed,
                    received = snapshot.urls_received,
                    new = snapshot.urls_new,
                    duplicate = snapshot.urls_duplicate,
                    dispatched = snapshot.urls_dispatched,
                    delayed = snapshot.urls_delayed,
                    recrawl_skipped = snapshot.urls_recrawl_skipped,
                    links_recorded = snapshot.links_recorded,
                    history_updates = snapshot.history_updates,
                    jobs = snapshot.active_jobs,
                    domains = snapshot.active_domains,
                    "Frontier metrics"
                );

                if let Some(ref graph) = link_graph {
                    let stats = graph.stats();
                    info!(
                        pages = stats.page_count,
                        links = stats.link_count,
                        avg_inbound = format!("{:.2}", stats.avg_inbound),
                        "LinkGraph stats"
                    );
                }

                if let Some(ref history) = url_history {
                    let stats = history.stats();
                    info!(
                        tracked_urls = stats.tracked_urls,
                        total_crawls = stats.total_crawls,
                        total_changes = stats.total_changes,
                        avg_change_rate = format!("{:.2}", stats.avg_change_rate),
                        "Recrawl stats"
                    );
                }
            }
        });

        let (dispatch_tx, dispatch_rx) = mpsc::channel::<ReadyUrl>(50000);

        let dispatcher_handle = self.start_dispatcher(dispatch_rx);
        let collector_handle = self.start_collector(dispatch_tx);
        let links_handle = self.start_links_consumer();
        let history_handle = self.start_history_consumer();
        let pagerank_handle = self.start_pagerank_computer();

        let result = self.process_messages().await;

        self.shutdown.store(true, Ordering::Relaxed);
        metrics_handle.abort();
        dispatcher_handle.abort();
        collector_handle.abort();
        if let Some(h) = links_handle {
            h.abort();
        }
        if let Some(h) = history_handle {
            h.abort();
        }
        if let Some(h) = pagerank_handle {
            h.abort();
        }

        result
    }

    async fn process_messages(&self) -> anyhow::Result<()> {
        self.consumer
            .process::<UrlMessage, _, _>(|msg, metadata| async move {
                if self.shutdown.load(Ordering::Relaxed) {
                    return Err(ScrapixError::Crawl("Service shutting down".into()));
                }

                self.metrics
                    .messages_consumed
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics.urls_received.fetch_add(1, Ordering::Relaxed);

                debug!(
                    url = %msg.url.url,
                    job_id = %msg.job_id,
                    depth = msg.url.depth,
                    partition = metadata.partition,
                    "Received URL"
                );

                let mut url = msg.url.clone();
                if let Some(ref scheduler) = self.recrawl_scheduler {
                    match scheduler.should_crawl(&url) {
                        RecrawlDecision::Crawl {
                            priority_boost,
                            reason,
                            ..
                        } => {
                            url.priority += priority_boost;
                            debug!(
                                url = %url.url,
                                reason = %reason,
                                priority_boost = priority_boost,
                                "Recrawl decision: crawl"
                            );
                        }
                        RecrawlDecision::Skip {
                            reason,
                            retry_after,
                        } => {
                            self.metrics
                                .urls_recrawl_skipped
                                .fetch_add(1, Ordering::Relaxed);
                            debug!(
                                url = %url.url,
                                reason = %reason,
                                retry_after_secs = retry_after.map(|d| d.as_secs()),
                                "Recrawl decision: skip"
                            );
                            return Ok(());
                        }
                    }
                }

                if let Some(ref graph) = self.link_graph {
                    let boost = graph.get_priority_boost(&url.url);
                    if boost > 0 {
                        url.priority += boost;
                        debug!(
                            url = %url.url,
                            pagerank_boost = boost,
                            "Applied PageRank priority boost"
                        );
                    }
                }

                let job = self.get_or_create_job(
                    &msg.job_id,
                    &msg.index_uid,
                    msg.url_patterns.clone(),
                    msg.meilisearch_url.clone(),
                    msg.meilisearch_api_key.clone(),
                );

                if job.try_add(url.clone()) {
                    self.metrics.urls_new.fetch_add(1, Ordering::Relaxed);
                    debug!(url = %url.url, job_id = %msg.job_id, "New URL added to queue");
                } else {
                    self.metrics.urls_duplicate.fetch_add(1, Ordering::Relaxed);
                    debug!(url = %url.url, job_id = %msg.job_id, "Duplicate URL filtered");
                }

                Ok(())
            })
            .await?;

        Ok(())
    }

    fn start_collector(&self, dispatch_tx: mpsc::Sender<ReadyUrl>) -> tokio::task::JoinHandle<()> {
        let jobs = Arc::clone(&self.jobs);
        let politeness = Arc::clone(&self.politeness);
        let shutdown = Arc::clone(&self.shutdown);
        let metrics = Arc::clone(&self.metrics);
        let batch_size = self.dispatch_batch_size;
        let interval = self.dispatch_interval;

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);

            while !shutdown.load(Ordering::Relaxed) {
                tick.tick().await;

                let jobs_snapshot: Vec<Arc<JobFrontier>> = {
                    let jobs = jobs.read();
                    jobs.values().cloned().collect()
                };

                for job in jobs_snapshot {
                    let urls = job.pop_ready(batch_size);

                    for url in urls {
                        let domain = extract_domain(&url.url);

                        if politeness.can_fetch(&domain) {
                            politeness.start_request(&domain);

                            let ready = ReadyUrl {
                                url,
                                job_id: job.job_id.clone(),
                                index_uid: job.index_uid.clone(),
                                domain,
                                url_patterns: job.url_patterns.clone(),
                                meilisearch_url: job.meilisearch_url.clone(),
                                meilisearch_api_key: job.meilisearch_api_key.clone(),
                            };

                            if dispatch_tx.send(ready).await.is_err() {
                                return;
                            }
                        } else {
                            metrics.urls_delayed.fetch_add(1, Ordering::Relaxed);
                            job.queue.push(url);
                        }
                    }
                }

                let domain_count = politeness.tracked_domains().len() as u64;
                metrics
                    .active_domains
                    .store(domain_count, Ordering::Relaxed);
            }
        })
    }

    fn start_dispatcher(
        &self,
        mut dispatch_rx: mpsc::Receiver<ReadyUrl>,
    ) -> tokio::task::JoinHandle<()> {
        let producer = Arc::clone(&self.producer);
        let shutdown = Arc::clone(&self.shutdown);
        let metrics = Arc::clone(&self.metrics);
        let politeness = Arc::clone(&self.politeness);

        tokio::spawn(async move {
            while !shutdown.load(Ordering::Relaxed) {
                match dispatch_rx.recv().await {
                    Some(ready) => {
                        let msg = if let Some(patterns) = ready.url_patterns {
                            UrlMessage::with_patterns(
                                ready.url,
                                &ready.job_id,
                                &ready.index_uid,
                                patterns,
                            )
                        } else {
                            UrlMessage::new(ready.url, &ready.job_id, &ready.index_uid)
                        }
                        .with_meilisearch(ready.meilisearch_url, ready.meilisearch_api_key);

                        match producer
                            .send(
                                topic_names::URL_PROCESSING,
                                Some(&msg.partition_key()),
                                &msg,
                            )
                            .await
                        {
                            Ok(_) => {
                                metrics.urls_dispatched.fetch_add(1, Ordering::Relaxed);
                                debug!(
                                    url = %msg.url.url,
                                    job_id = %msg.job_id,
                                    topic = topic_names::URL_PROCESSING,
                                    "Dispatched URL for crawling"
                                );

                                politeness.complete_request(&ready.domain);
                            }
                            Err(e) => {
                                error!(
                                    url = %msg.url.url,
                                    job_id = %msg.job_id,
                                    error = %e,
                                    "Failed to dispatch URL"
                                );

                                politeness.failed_request(&ready.domain, false);
                            }
                        }
                    }
                    None => {
                        break;
                    }
                }
            }
        })
    }

    fn start_links_consumer(&self) -> Option<tokio::task::JoinHandle<()>> {
        let consumer = self.links_consumer.clone()?;
        let link_graph = self.link_graph.clone()?;
        let metrics = Arc::clone(&self.metrics);
        let shutdown = Arc::clone(&self.shutdown);

        Some(tokio::spawn(async move {
            while !shutdown.load(Ordering::Relaxed) {
                match consumer
                    .poll_one::<LinksMessage>(Duration::from_millis(100))
                    .await
                {
                    Ok(Some(msg)) => {
                        link_graph.record_links(&msg.source_url, msg.target_urls.clone());
                        metrics
                            .links_recorded
                            .fetch_add(msg.target_urls.len() as u64, Ordering::Relaxed);
                        debug!(
                            source = %msg.source_url,
                            links_count = msg.target_urls.len(),
                            "Recorded links in graph"
                        );
                    }
                    Ok(None) => {}
                    Err(e) => {
                        debug!(error = %e, "Error polling links topic");
                    }
                }
            }
        }))
    }

    fn start_history_consumer(&self) -> Option<tokio::task::JoinHandle<()>> {
        let consumer = self.history_consumer.clone()?;
        let url_history = self.url_history.clone()?;
        let metrics = Arc::clone(&self.metrics);
        let shutdown = Arc::clone(&self.shutdown);

        Some(tokio::spawn(async move {
            while !shutdown.load(Ordering::Relaxed) {
                match consumer
                    .poll_one::<CrawlHistoryMessage>(Duration::from_millis(100))
                    .await
                {
                    Ok(Some(msg)) => {
                        let mut record = CrawlRecord::new().with_status(msg.status);

                        if let Some(etag) = msg.etag {
                            record = record.with_etag(etag);
                        }
                        if let Some(last_modified) = msg.last_modified {
                            record = record.with_last_modified(last_modified);
                        }
                        if let Some(content_hash) = msg.content_hash {
                            record = record.with_content_hash(content_hash);
                        }
                        if let Some(content_length) = msg.content_length {
                            record = record.with_content_length(content_length);
                        }

                        url_history.record_crawl(&msg.url, record);
                        metrics.history_updates.fetch_add(1, Ordering::Relaxed);
                        debug!(
                            url = %msg.url,
                            content_changed = msg.content_changed,
                            "Recorded crawl history"
                        );
                    }
                    Ok(None) => {}
                    Err(e) => {
                        debug!(error = %e, "Error polling history topic");
                    }
                }
            }
        }))
    }

    fn start_pagerank_computer(&self) -> Option<tokio::task::JoinHandle<()>> {
        let link_graph = self.link_graph.clone()?;
        let shutdown = Arc::clone(&self.shutdown);
        let interval = self.linkgraph_compute_interval;

        Some(tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);

            while !shutdown.load(Ordering::Relaxed) {
                tick.tick().await;

                let start = std::time::Instant::now();
                link_graph.compute_scores();
                let duration = start.elapsed();

                let stats = link_graph.stats();
                info!(
                    pages = stats.page_count,
                    links = stats.link_count,
                    duration_ms = duration.as_millis(),
                    max_score = format!("{:.6}", stats.max_score),
                    "Recomputed PageRank scores"
                );
            }
        }))
    }

    fn shutdown(&self) {
        info!(instance_id = %self.instance_id, "Initiating graceful shutdown");
        self.shutdown.store(true, Ordering::Relaxed);
    }

    fn print_stats(&self) {
        let metrics = self.metrics.snapshot();
        info!(
            consumed = metrics.messages_consumed,
            received = metrics.urls_received,
            new = metrics.urls_new,
            duplicate = metrics.urls_duplicate,
            dispatched = metrics.urls_dispatched,
            delayed = metrics.urls_delayed,
            "Final frontier metrics"
        );

        let jobs = self.jobs.read();
        for job in jobs.values() {
            let stats = job.stats();
            info!(
                job_id = %stats.job_id,
                received = stats.urls_received,
                deduplicated = stats.urls_deduplicated,
                dispatched = stats.urls_dispatched,
                pending = stats.urls_pending,
                bloom_items = stats.dedup_stats.items_count,
                bloom_memory_mb = stats.dedup_stats.estimated_memory_bytes / (1024 * 1024),
                "Job stats"
            );
        }
    }
}

/// Run the frontier service with the given arguments.
pub async fn run(args: Args) -> anyhow::Result<()> {
    info!(
        brokers = %args.brokers,
        group_id = %args.group_id,
        bloom_capacity = args.bloom_capacity,
        domain_delay_ms = args.domain_delay_ms,
        "Starting Scrapix frontier service"
    );

    let service = Arc::new(FrontierService::new(&args).await?);

    let service_shutdown = service.clone();
    let shutdown_handle = tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!(error = %e, "Failed to listen for ctrl+c");
        }
        info!("Received shutdown signal");
        service_shutdown.shutdown();
    });

    let result = service.run().await;

    shutdown_handle.abort();
    service.print_stats();

    result
}

/// Run the frontier service using pre-built message bus trait objects.
///
/// Used by `scrapix all` to run the frontier in-process alongside other services,
/// sharing an in-process channel bus instead of Kafka.
pub async fn run_with_bus(
    args: Args,
    producer: Arc<AnyProducer>,
    main_consumer: Arc<AnyConsumer>,
    links_consumer: Option<Arc<AnyConsumer>>,
    history_consumer: Option<Arc<AnyConsumer>>,
) -> anyhow::Result<()> {
    info!(
        bloom_capacity = args.bloom_capacity,
        domain_delay_ms = args.domain_delay_ms,
        "Starting Scrapix frontier service (in-process bus)"
    );

    let service = Arc::new(
        FrontierService::with_bus(&args, producer, main_consumer, links_consumer, history_consumer)
            .await?,
    );

    let result = service.run().await;

    service.print_stats();

    result
}
