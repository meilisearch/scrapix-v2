//! All-in-one orchestrator for running all services in a single process.
//!
//! Creates an in-process channel bus (or Kafka if `--kafka-brokers` is set),
//! then spawns API, Frontier, Crawler, and Content as concurrent tokio tasks.

use std::sync::Arc;

use clap::Parser;
use tracing::{error, info};

use scrapix_queue::{topic_names, AnyConsumer, AnyProducer, ChannelBus};

/// Arguments for running all services in a single process.
#[derive(Parser, Debug)]
#[command(name = "all")]
#[command(about = "Run all Scrapix services in a single process")]
pub struct AllArgs {
    // === API ===
    /// API server host
    #[arg(long, env = "HOST", default_value = "0.0.0.0")]
    pub host: String,

    /// API server port
    #[arg(long, env = "PORT", default_value = "8080")]
    pub port: u16,

    // === Meilisearch ===
    /// Meilisearch server URL
    #[arg(long, env = "MEILISEARCH_URL", default_value = "http://localhost:7700")]
    pub meilisearch_url: String,

    /// Meilisearch API key
    #[arg(long, env = "MEILISEARCH_API_KEY", default_value = "masterKey")]
    pub meilisearch_key: String,

    // === Workers ===
    /// Crawler concurrency (concurrent fetchers)
    #[arg(long, env = "CRAWLER_CONCURRENCY", default_value = "50")]
    pub crawler_concurrency: usize,

    /// Content worker concurrency
    #[arg(long, env = "CONTENT_CONCURRENCY", default_value = "10")]
    pub content_concurrency: usize,

    // === Optional Kafka ===
    /// Kafka/Redpanda brokers. If set, uses Kafka instead of in-process channels.
    #[arg(long, env = "KAFKA_BROKERS")]
    pub kafka_brokers: Option<String>,

    // === Database (optional) ===
    /// PostgreSQL database URL (for auth/cron features)
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: Option<String>,

    /// JWT secret for auth
    #[arg(long, env = "JWT_SECRET", default_value = "scrapix-dev-secret")]
    pub jwt_secret: String,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,
}

pub async fn run_all(args: AllArgs) -> anyhow::Result<()> {
    if let Some(ref brokers) = args.kafka_brokers {
        info!(brokers = %brokers, "Running all services with Kafka message bus");
        run_all_kafka(&args, brokers).await
    } else {
        info!("Running all services with in-process channel bus");
        run_all_channels(&args).await
    }
}

async fn run_all_channels(args: &AllArgs) -> anyhow::Result<()> {
    let bus = ChannelBus::new();

    // Create producers (all services share the same bus)
    let api_producer = AnyProducer::channel(bus.producer());
    let frontier_producer = Arc::new(AnyProducer::channel(bus.producer()));
    let crawler_producer = AnyProducer::channel(bus.producer());
    let content_producer = Arc::new(AnyProducer::channel(bus.producer()));

    // Create consumers for each service
    // API event consumer
    let api_event_consumer = AnyConsumer::channel(bus.consumer());
    api_event_consumer.subscribe(&[topic_names::EVENTS])?;

    // Frontier main consumer (URL_FRONTIER)
    let frontier_consumer = Arc::new(AnyConsumer::channel(bus.consumer()));
    frontier_consumer.subscribe(&[topic_names::URL_FRONTIER])?;

    // Crawler consumer (URL_PROCESSING)
    let crawler_consumer = AnyConsumer::channel(bus.consumer());
    crawler_consumer.subscribe(&[topic_names::URL_PROCESSING])?;

    // Content consumer (PAGES_RAW)
    let content_consumer = Arc::new(AnyConsumer::channel(bus.consumer()));
    content_consumer.subscribe(&[topic_names::PAGES_RAW])?;

    // Build service-specific args
    let api_args = scrapix_api::Args {
        host: args.host.clone(),
        port: args.port,
        brokers: String::new(), // unused with channel bus
        database_url: args.database_url.clone(),
        jwt_secret: args.jwt_secret.clone(),
        max_jobs: 1000,
        verbose: args.verbose,
    };

    let frontier_args = scrapix_frontier_service::Args {
        brokers: String::new(),
        group_id: "scrapix-frontier".to_string(),
        bloom_capacity: 10_000_000,
        bloom_fp_rate: 0.01,
        domain_delay_ms: 50,
        concurrent_per_domain: 50,
        dispatch_batch_size: 2000,
        dispatch_interval_ms: 20,
        max_pending_per_job: 1_000_000,
        instance_id: Some("all-in-one".to_string()),
        verbose: args.verbose,
        enable_linkgraph: false,
        linkgraph_damping: 0.85,
        linkgraph_max_boost: 50,
        linkgraph_max_pages: 10_000_000,
        linkgraph_compute_interval: 300,
        enable_recrawl: false,
        recrawl_min_age: 3600,
        recrawl_max_age: 604800,
        recrawl_max_urls: 10_000_000,
    };

    let crawler_args = scrapix_worker_crawler::Args {
        brokers: String::new(),
        group_id: "scrapix-crawlers".to_string(),
        concurrency: args.crawler_concurrency,
        user_agent: "Scrapix/1.0 (compatible; +https://github.com/quentindequelen/scrapix)"
            .to_string(),
        timeout: 30,
        max_retries: 3,
        follow_external: false,
        max_depth: 100,
        max_body_size_mb: 10,
        respect_robots: true,
        worker_id: Some("all-in-one-crawler".to_string()),
        dns_cache: true,
        dns_cache_ttl: 300,
        link_graph: false,
        link_graph_interval: 1000,
        publish_links: false,
        incremental_crawl: true,
        browser_render: false,
        browser_render_patterns: None,
        chrome_path: None,
        browser_timeout: 30,
        browser_concurrency: 5,
        browser_headless: true,
        verbose: args.verbose,
        rocksdb_path: "./data/crawler-rocksdb".to_string(),
        sitemap_discovery: true,
        max_sitemap_urls: 10000,
    };

    let content_args = build_content_args(args, String::new());

    info!("Spawning all services as concurrent tasks...");

    // Spawn all 4 services
    let api_handle = tokio::spawn(async move {
        if let Err(e) = scrapix_api::run_with_bus(api_args, api_producer, api_event_consumer).await
        {
            error!(error = %e, "API server failed");
        }
    });

    let frontier_handle = tokio::spawn(async move {
        if let Err(e) = scrapix_frontier_service::run_with_bus(
            frontier_args,
            frontier_producer,
            frontier_consumer,
            None, // links consumer
            None, // history consumer
        )
        .await
        {
            error!(error = %e, "Frontier service failed");
        }
    });

    let crawler_handle = tokio::spawn(async move {
        if let Err(e) =
            scrapix_worker_crawler::run_with_bus(crawler_args, crawler_producer, crawler_consumer)
                .await
        {
            error!(error = %e, "Crawler worker failed");
        }
    });

    let content_handle = tokio::spawn(async move {
        if let Err(e) =
            scrapix_worker_content::run_with_bus(content_args, content_consumer, content_producer)
                .await
        {
            error!(error = %e, "Content worker failed");
        }
    });

    info!(
        host = %args.host,
        port = args.port,
        crawler_concurrency = args.crawler_concurrency,
        content_concurrency = args.content_concurrency,
        "All services started. Press Ctrl+C to stop."
    );

    // Wait for ctrl+c
    tokio::signal::ctrl_c().await?;
    info!("Received shutdown signal, stopping all services...");

    // Abort all tasks (each service handles its own graceful shutdown internally)
    api_handle.abort();
    frontier_handle.abort();
    crawler_handle.abort();
    content_handle.abort();

    // Give services 5 seconds to clean up
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    info!("All services stopped.");
    Ok(())
}

fn build_content_args(args: &AllArgs, brokers: String) -> scrapix_worker_content::Args {
    scrapix_worker_content::Args {
        brokers,
        group_id: "scrapix-content".to_string(),
        concurrency: args.content_concurrency,
        meilisearch_url: args.meilisearch_url.clone(),
        meilisearch_key: Some(args.meilisearch_key.clone()),
        default_index: "scrapix".to_string(),
        extract_content: true,
        convert_markdown: true,
        detect_language: true,
        extract_schema: true,
        min_content_length: 100,
        publish_to_kafka: false,
        publish_history: false,
        skip_meilisearch: false,
        batch_size: 2000,
        worker_id: Some("all-in-one-content".to_string()),
        verbose: args.verbose,
        enable_summary: false,
        summary_model: "gpt-5-nano".to_string(),
        enable_extraction: false,
        extraction_prompt: None,
        extraction_model: "gpt-5-nano".to_string(),
        ai_max_tokens: 1000,
        ai_concurrency: 5,
        enable_block_split: false,
        block_split_min_level: 2,
        block_split_max_level: 4,
        block_split_min_length: 50,
        enable_dedup: false,
        dedup_use_simhash: true,
        dedup_simhash_threshold: 3,
        dedup_minhash_threshold: 0.85,
        dedup_max_fingerprints: 10_000_000,
    }
}

async fn run_all_kafka(args: &AllArgs, brokers: &str) -> anyhow::Result<()> {
    // When Kafka is specified, just build Kafka-backed producers/consumers
    use scrapix_queue::{ConsumerBuilder, ProducerBuilder};

    let api_producer: AnyProducer = ProducerBuilder::new(brokers)
        .client_id("scrapix-all-api")
        .compression("lz4")
        .build()?
        .into();

    let api_event_consumer: AnyConsumer = {
        let c = ConsumerBuilder::new(brokers, "scrapix-all-api-events")
            .client_id("scrapix-all-api-events")
            .auto_offset_reset("latest")
            .build()?;
        c.subscribe(&[topic_names::EVENTS])?;
        c.into()
    };

    let frontier_producer: Arc<AnyProducer> = Arc::new(
        ProducerBuilder::new(brokers)
            .client_id("scrapix-all-frontier")
            .compression("lz4")
            .build()?
            .into(),
    );

    let frontier_consumer: Arc<AnyConsumer> = Arc::new({
        let c = ConsumerBuilder::new(brokers, "scrapix-all-frontier")
            .client_id("scrapix-all-frontier")
            .auto_offset_reset("earliest")
            .build()?;
        c.subscribe(&[topic_names::URL_FRONTIER])?;
        AnyConsumer::from(c)
    });

    let crawler_producer: AnyProducer = ProducerBuilder::new(brokers)
        .client_id("scrapix-all-crawler")
        .compression("lz4")
        .build()?
        .into();

    let crawler_consumer: AnyConsumer = {
        let c = ConsumerBuilder::new(brokers, "scrapix-all-crawlers")
            .client_id("scrapix-all-crawler")
            .auto_offset_reset("earliest")
            .build()?;
        c.subscribe(&[topic_names::URL_PROCESSING])?;
        c.into()
    };

    let content_producer: Arc<AnyProducer> = Arc::new(
        ProducerBuilder::new(brokers)
            .client_id("scrapix-all-content")
            .compression("lz4")
            .build()?
            .into(),
    );

    let content_consumer: Arc<AnyConsumer> = Arc::new({
        let c = ConsumerBuilder::new(brokers, "scrapix-all-content")
            .client_id("scrapix-all-content")
            .auto_offset_reset("earliest")
            .build()?;
        c.subscribe(&[topic_names::PAGES_RAW])?;
        AnyConsumer::from(c)
    });

    // Build the same args as channel mode
    let api_args = scrapix_api::Args {
        host: args.host.clone(),
        port: args.port,
        brokers: brokers.to_string(),
        database_url: args.database_url.clone(),
        jwt_secret: args.jwt_secret.clone(),
        max_jobs: 1000,
        verbose: args.verbose,
    };

    let frontier_args = scrapix_frontier_service::Args {
        brokers: brokers.to_string(),
        group_id: "scrapix-all-frontier".to_string(),
        bloom_capacity: 10_000_000,
        bloom_fp_rate: 0.01,
        domain_delay_ms: 50,
        concurrent_per_domain: 50,
        dispatch_batch_size: 2000,
        dispatch_interval_ms: 20,
        max_pending_per_job: 1_000_000,
        instance_id: Some("all-in-one".to_string()),
        verbose: args.verbose,
        enable_linkgraph: false,
        linkgraph_damping: 0.85,
        linkgraph_max_boost: 50,
        linkgraph_max_pages: 10_000_000,
        linkgraph_compute_interval: 300,
        enable_recrawl: false,
        recrawl_min_age: 3600,
        recrawl_max_age: 604800,
        recrawl_max_urls: 10_000_000,
    };

    let crawler_args = scrapix_worker_crawler::Args {
        brokers: brokers.to_string(),
        group_id: "scrapix-all-crawlers".to_string(),
        concurrency: args.crawler_concurrency,
        user_agent: "Scrapix/1.0 (compatible; +https://github.com/quentindequelen/scrapix)"
            .to_string(),
        timeout: 30,
        max_retries: 3,
        follow_external: false,
        max_depth: 100,
        max_body_size_mb: 10,
        respect_robots: true,
        worker_id: Some("all-in-one-crawler".to_string()),
        dns_cache: true,
        dns_cache_ttl: 300,
        link_graph: false,
        link_graph_interval: 1000,
        publish_links: false,
        incremental_crawl: true,
        browser_render: false,
        browser_render_patterns: None,
        chrome_path: None,
        browser_timeout: 30,
        browser_concurrency: 5,
        browser_headless: true,
        verbose: args.verbose,
        rocksdb_path: "./data/crawler-rocksdb".to_string(),
        sitemap_discovery: true,
        max_sitemap_urls: 10000,
    };

    let content_args = build_content_args(args, brokers.to_string());

    info!("Spawning all services with Kafka bus...");

    let api_handle = tokio::spawn(async move {
        if let Err(e) = scrapix_api::run_with_bus(api_args, api_producer, api_event_consumer).await
        {
            error!(error = %e, "API server failed");
        }
    });

    let frontier_handle = tokio::spawn(async move {
        if let Err(e) = scrapix_frontier_service::run_with_bus(
            frontier_args,
            frontier_producer,
            frontier_consumer,
            None,
            None,
        )
        .await
        {
            error!(error = %e, "Frontier service failed");
        }
    });

    let crawler_handle = tokio::spawn(async move {
        if let Err(e) =
            scrapix_worker_crawler::run_with_bus(crawler_args, crawler_producer, crawler_consumer)
                .await
        {
            error!(error = %e, "Crawler worker failed");
        }
    });

    let content_handle = tokio::spawn(async move {
        if let Err(e) =
            scrapix_worker_content::run_with_bus(content_args, content_consumer, content_producer)
                .await
        {
            error!(error = %e, "Content worker failed");
        }
    });

    info!(
        host = %args.host,
        port = args.port,
        brokers = %brokers,
        "All services started with Kafka. Press Ctrl+C to stop."
    );

    tokio::signal::ctrl_c().await?;
    info!("Received shutdown signal, stopping all services...");

    api_handle.abort();
    frontier_handle.abort();
    crawler_handle.abort();
    content_handle.abort();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    info!("All services stopped.");
    Ok(())
}
