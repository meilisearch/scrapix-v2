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
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use scrapix_core::{CrawlUrl, ScrapixError};
use scrapix_frontier::{
    extract_domain, DedupConfig, PolitenessConfig, PolitenessScheduler, PriorityConfig,
    PriorityQueue, UrlDedup,
};
use scrapix_queue::{
    topic_names, ConsumerBuilder, KafkaConsumer, KafkaProducer, ProducerBuilder, UrlMessage,
};

/// Scrapix Frontier Service
#[derive(Parser, Debug)]
#[command(name = "scrapix-frontier-service")]
#[command(
    version,
    about = "URL frontier management service with deduplication and politeness"
)]
struct Args {
    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    brokers: String,

    /// Consumer group ID
    #[arg(
        short,
        long,
        env = "KAFKA_GROUP_ID",
        default_value = "scrapix-frontier"
    )]
    group_id: String,

    /// Bloom filter expected capacity per job
    #[arg(long, env = "BLOOM_CAPACITY", default_value = "10000000")]
    bloom_capacity: usize,

    /// Bloom filter false positive rate
    #[arg(long, env = "BLOOM_FP_RATE", default_value = "0.01")]
    bloom_fp_rate: f64,

    /// Default delay between requests to the same domain (ms)
    #[arg(long, env = "DOMAIN_DELAY_MS", default_value = "1000")]
    domain_delay_ms: u64,

    /// Maximum concurrent requests per domain
    #[arg(long, env = "CONCURRENT_PER_DOMAIN", default_value = "2")]
    concurrent_per_domain: usize,

    /// URL dispatch batch size
    #[arg(long, env = "DISPATCH_BATCH_SIZE", default_value = "100")]
    dispatch_batch_size: usize,

    /// Dispatch interval (ms)
    #[arg(long, env = "DISPATCH_INTERVAL_MS", default_value = "100")]
    dispatch_interval_ms: u64,

    /// Maximum pending URLs per job
    #[arg(long, env = "MAX_PENDING_PER_JOB", default_value = "1000000")]
    max_pending_per_job: usize,

    /// Service instance ID
    #[arg(long, env = "INSTANCE_ID")]
    instance_id: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
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
}

impl JobFrontier {
    fn new(
        job_id: &str,
        index_uid: &str,
        dedup_config: &DedupConfig,
        priority_config: &PriorityConfig,
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
        }
    }

    /// Try to add a URL, returns true if added (not a duplicate)
    fn try_add(&self, url: CrawlUrl) -> bool {
        self.urls_received.fetch_add(1, Ordering::Relaxed);

        if self.dedup.check_and_mark(&url.url) {
            // Already seen
            self.urls_deduplicated.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            // New URL, add to priority queue
            self.queue.push(url);
            true
        }
    }

    /// Pop URLs ready for dispatch
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

/// Statistics for a job
#[derive(Debug)]
struct JobStats {
    job_id: String,
    urls_received: u64,
    urls_deduplicated: u64,
    urls_dispatched: u64,
    urls_pending: u64,
    dedup_stats: scrapix_frontier::DedupStats,
}

/// Service metrics
#[derive(Debug, Default)]
struct ServiceMetrics {
    messages_consumed: AtomicU64,
    urls_received: AtomicU64,
    urls_new: AtomicU64,
    urls_duplicate: AtomicU64,
    urls_dispatched: AtomicU64,
    urls_delayed: AtomicU64,
    active_jobs: AtomicU64,
    active_domains: AtomicU64,
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
            active_jobs: self.active_jobs.load(Ordering::Relaxed),
            active_domains: self.active_domains.load(Ordering::Relaxed),
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
    active_jobs: u64,
    active_domains: u64,
}

/// URL ready for dispatch with metadata
struct ReadyUrl {
    url: CrawlUrl,
    job_id: String,
    index_uid: String,
    domain: String,
}

/// The frontier service
struct FrontierService {
    consumer: KafkaConsumer,
    producer: Arc<KafkaProducer>,
    jobs: Arc<RwLock<HashMap<String, Arc<JobFrontier>>>>,
    politeness: Arc<PolitenessScheduler>,
    metrics: Arc<ServiceMetrics>,
    shutdown: Arc<AtomicBool>,
    instance_id: String,
    dedup_config: DedupConfig,
    priority_config: PriorityConfig,
    dispatch_batch_size: usize,
    dispatch_interval: Duration,
}

impl FrontierService {
    async fn new(args: &Args) -> anyhow::Result<Self> {
        let instance_id = args
            .instance_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(instance_id = %instance_id, "Initializing frontier service");

        // Create Kafka consumer
        let consumer = ConsumerBuilder::new(&args.brokers, &args.group_id)
            .client_id(format!("scrapix-frontier-{}", instance_id))
            .auto_offset_reset("earliest")
            .build()?;

        // Subscribe to frontier topic
        consumer.subscribe(&[topic_names::URL_FRONTIER])?;
        info!(
            topic = topic_names::URL_FRONTIER,
            "Subscribed to frontier topic"
        );

        // Create Kafka producer
        let producer = ProducerBuilder::new(&args.brokers)
            .client_id(format!("scrapix-frontier-{}-producer", instance_id))
            .compression("lz4")
            .build()?;

        // Create politeness scheduler
        let politeness_config = PolitenessConfig {
            default_delay_ms: args.domain_delay_ms,
            min_delay_ms: 100,
            max_delay_ms: 30_000,
            respect_robots_delay: true,
            robots_delay_multiplier: 1.0,
            concurrent_per_domain: args.concurrent_per_domain,
        };
        let politeness = PolitenessScheduler::new(politeness_config);

        // Dedup and priority configs
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

        Ok(Self {
            consumer,
            producer: Arc::new(producer),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            politeness: Arc::new(politeness),
            metrics: Arc::new(ServiceMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            instance_id,
            dedup_config,
            priority_config,
            dispatch_batch_size: args.dispatch_batch_size,
            dispatch_interval: Duration::from_millis(args.dispatch_interval_ms),
        })
    }

    /// Get or create a job frontier
    fn get_or_create_job(&self, job_id: &str, index_uid: &str) -> Arc<JobFrontier> {
        // Fast path: check if job exists
        {
            let jobs = self.jobs.read();
            if let Some(job) = jobs.get(job_id) {
                return job.clone();
            }
        }

        // Slow path: create new job
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
                ))
            })
            .clone()
    }

    /// Run the frontier service
    async fn run(&self) -> anyhow::Result<()> {
        info!(instance_id = %self.instance_id, "Starting frontier service main loop");

        // Start metrics reporter
        let metrics = self.metrics.clone();
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
                    jobs = snapshot.active_jobs,
                    domains = snapshot.active_domains,
                    "Frontier metrics"
                );
            }
        });

        // Channel for URLs ready to dispatch
        let (dispatch_tx, dispatch_rx) = mpsc::channel::<ReadyUrl>(10000);

        // Start dispatcher task
        let dispatcher_handle = self.start_dispatcher(dispatch_rx);

        // Start URL collector task (collects from job queues and checks politeness)
        let collector_handle = self.start_collector(dispatch_tx);

        // Process incoming messages
        let result = self.process_messages().await;

        // Cleanup
        self.shutdown.store(true, Ordering::Relaxed);
        metrics_handle.abort();
        dispatcher_handle.abort();
        collector_handle.abort();

        result
    }

    /// Process incoming messages from Kafka
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

                // Get or create job frontier
                let job = self.get_or_create_job(&msg.job_id, &msg.index_uid);

                // Try to add URL (deduplication happens here)
                if job.try_add(msg.url.clone()) {
                    self.metrics.urls_new.fetch_add(1, Ordering::Relaxed);
                    debug!(url = %msg.url.url, job_id = %msg.job_id, "New URL added to queue");
                } else {
                    self.metrics.urls_duplicate.fetch_add(1, Ordering::Relaxed);
                    debug!(url = %msg.url.url, job_id = %msg.job_id, "Duplicate URL filtered");
                }

                Ok(())
            })
            .await?;

        Ok(())
    }

    /// Start the URL collector task that checks politeness and sends URLs to dispatcher
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

                // Collect URLs from all jobs
                let jobs_snapshot: Vec<Arc<JobFrontier>> = {
                    let jobs = jobs.read();
                    jobs.values().cloned().collect()
                };

                for job in jobs_snapshot {
                    // Get ready URLs from this job
                    let urls = job.pop_ready(batch_size);

                    for url in urls {
                        let domain = extract_domain(&url.url);

                        // Check politeness
                        if politeness.can_fetch(&domain) {
                            // Mark request as starting
                            politeness.start_request(&domain);

                            let ready = ReadyUrl {
                                url,
                                job_id: job.job_id.clone(),
                                index_uid: job.index_uid.clone(),
                                domain,
                            };

                            if dispatch_tx.send(ready).await.is_err() {
                                // Channel closed
                                return;
                            }
                        } else {
                            // Not ready yet, push back to queue
                            metrics.urls_delayed.fetch_add(1, Ordering::Relaxed);
                            job.queue.push(url);
                        }
                    }
                }

                // Update active domains count
                let domain_count = politeness.tracked_domains().len() as u64;
                metrics
                    .active_domains
                    .store(domain_count, Ordering::Relaxed);
            }
        })
    }

    /// Start the dispatcher task that publishes URLs to Kafka
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
                        let msg = UrlMessage::new(ready.url, &ready.job_id, &ready.index_uid);

                        // Publish to processing topic
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

                                // Mark request as complete for politeness tracking
                                politeness.complete_request(&ready.domain);
                            }
                            Err(e) => {
                                error!(
                                    url = %msg.url.url,
                                    job_id = %msg.job_id,
                                    error = %e,
                                    "Failed to dispatch URL"
                                );

                                // Mark as failed for adaptive backoff
                                politeness.failed_request(&ready.domain, false);
                            }
                        }
                    }
                    None => {
                        // Channel closed
                        break;
                    }
                }
            }
        })
    }

    /// Graceful shutdown
    fn shutdown(&self) {
        info!(instance_id = %self.instance_id, "Initiating graceful shutdown");
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Print final statistics
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

        // Print per-job stats
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
        brokers = %args.brokers,
        group_id = %args.group_id,
        bloom_capacity = args.bloom_capacity,
        domain_delay_ms = args.domain_delay_ms,
        "Starting Scrapix frontier service"
    );

    // Create and run service
    let service = Arc::new(FrontierService::new(&args).await?);

    // Setup shutdown handler
    let service_shutdown = service.clone();
    let shutdown_handle = tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!(error = %e, "Failed to listen for ctrl+c");
        }
        info!("Received shutdown signal");
        service_shutdown.shutdown();
    });

    // Run the service
    let result = service.run().await;

    // Cleanup
    shutdown_handle.abort();

    // Print final stats
    service.print_stats();

    result
}
