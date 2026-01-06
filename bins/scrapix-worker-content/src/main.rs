//! Scrapix Content Worker
//!
//! Processes raw HTML pages into structured documents for indexing.
//!
//! ## Responsibilities
//!
//! 1. Consume raw pages from the `scrapix.pages.raw` topic
//! 2. Parse HTML to extract content, title, metadata
//! 3. Convert content to Markdown
//! 4. Detect language
//! 5. Optionally split into blocks by headings
//! 6. Publish processed documents to `scrapix.documents` topic
//! 7. Index documents directly to Meilisearch

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use scrapix_ai::{AiClient, AiClientConfig, AiService};
use scrapix_core::{Document, RawPage, ScrapixError};
use scrapix_parser::{HtmlParser, HtmlParserConfig};
use scrapix_queue::{
    topic_names, ConsumerBuilder, CrawlEvent, DocumentMessage, KafkaConsumer, KafkaProducer,
    ProducerBuilder, RawPageMessage,
};
use scrapix_storage::{MeilisearchStorage, MeilisearchStorageBuilder};

/// Content processing worker for parsing HTML and creating documents
#[derive(Parser, Debug)]
#[command(name = "scrapix-worker-content")]
#[command(version, about = "Content processing worker")]
struct Args {
    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    brokers: String,

    /// Consumer group ID
    #[arg(short, long, env = "KAFKA_GROUP_ID", default_value = "scrapix-content")]
    group_id: String,

    /// Number of concurrent processors
    #[arg(short, long, env = "CONCURRENCY", default_value = "5")]
    concurrency: usize,

    /// Meilisearch URL
    #[arg(long, env = "MEILISEARCH_URL", default_value = "http://localhost:7700")]
    meilisearch_url: String,

    /// Meilisearch API key
    #[arg(long, env = "MEILISEARCH_API_KEY")]
    meilisearch_key: Option<String>,

    /// Default index UID (can be overridden by message)
    #[arg(long, env = "MEILISEARCH_INDEX", default_value = "documents")]
    default_index: String,

    /// Enable content extraction (readability algorithm)
    #[arg(long, env = "EXTRACT_CONTENT", default_value = "true")]
    extract_content: bool,

    /// Enable Markdown conversion
    #[arg(long, env = "CONVERT_MARKDOWN", default_value = "true")]
    convert_markdown: bool,

    /// Enable language detection
    #[arg(long, env = "DETECT_LANGUAGE", default_value = "true")]
    detect_language: bool,

    /// Enable schema.org extraction
    #[arg(long, env = "EXTRACT_SCHEMA", default_value = "true")]
    extract_schema: bool,

    /// Minimum content length to consider valid (characters)
    #[arg(long, env = "MIN_CONTENT_LENGTH", default_value = "100")]
    min_content_length: usize,

    /// Publish documents to Kafka topic (in addition to Meilisearch)
    #[arg(long, env = "PUBLISH_TO_KAFKA")]
    publish_to_kafka: bool,

    /// Skip Meilisearch indexing (only publish to Kafka)
    #[arg(long, env = "SKIP_MEILISEARCH")]
    skip_meilisearch: bool,

    /// Batch size for Meilisearch indexing
    #[arg(long, env = "BATCH_SIZE", default_value = "100")]
    batch_size: usize,

    /// Worker ID (for logging/metrics)
    #[arg(long, env = "WORKER_ID")]
    worker_id: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    // === AI ENRICHMENT OPTIONS ===
    /// OpenAI API key (required for AI features)
    #[arg(long, env = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,

    /// OpenAI API base URL (for compatible APIs like Azure, local models)
    #[arg(long, env = "OPENAI_API_BASE")]
    openai_api_base: Option<String>,

    /// Enable AI summarization (generates ai_summary field)
    #[arg(long, env = "ENABLE_SUMMARY")]
    enable_summary: bool,

    /// Summary model to use
    #[arg(long, env = "SUMMARY_MODEL", default_value = "gpt-4o-mini")]
    summary_model: String,

    /// Enable AI extraction with custom prompt (generates ai_extraction field)
    #[arg(long, env = "ENABLE_EXTRACTION")]
    enable_extraction: bool,

    /// Custom extraction prompt (required if enable_extraction is true)
    /// Use {content} as placeholder for the page content
    #[arg(long, env = "EXTRACTION_PROMPT")]
    extraction_prompt: Option<String>,

    /// Extraction model to use
    #[arg(long, env = "EXTRACTION_MODEL", default_value = "gpt-4o-mini")]
    extraction_model: String,

    /// Maximum tokens for AI responses
    #[arg(long, env = "AI_MAX_TOKENS", default_value = "1000")]
    ai_max_tokens: u32,

    /// Maximum concurrent AI requests
    #[arg(long, env = "AI_CONCURRENCY", default_value = "5")]
    ai_concurrency: usize,
}

/// Worker metrics for monitoring
#[derive(Debug, Default)]
struct WorkerMetrics {
    pages_processed: AtomicU64,
    pages_succeeded: AtomicU64,
    pages_failed: AtomicU64,
    pages_skipped: AtomicU64,
    documents_created: AtomicU64,
    documents_indexed: AtomicU64,
    bytes_processed: AtomicU64,
    active_processors: AtomicU64,
}

impl WorkerMetrics {
    fn new() -> Self {
        Self::default()
    }

    fn record_success(&self, bytes: u64, doc_count: u64) {
        self.pages_processed.fetch_add(1, Ordering::Relaxed);
        self.pages_succeeded.fetch_add(1, Ordering::Relaxed);
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
        self.documents_created
            .fetch_add(doc_count, Ordering::Relaxed);
    }

    fn record_failure(&self) {
        self.pages_processed.fetch_add(1, Ordering::Relaxed);
        self.pages_failed.fetch_add(1, Ordering::Relaxed);
    }

    fn record_skipped(&self) {
        self.pages_processed.fetch_add(1, Ordering::Relaxed);
        self.pages_skipped.fetch_add(1, Ordering::Relaxed);
    }

    fn record_indexed(&self, count: u64) {
        self.documents_indexed.fetch_add(count, Ordering::Relaxed);
    }

    fn processor_started(&self) {
        self.active_processors.fetch_add(1, Ordering::Relaxed);
    }

    fn processor_completed(&self) {
        self.active_processors.fetch_sub(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            pages_processed: self.pages_processed.load(Ordering::Relaxed),
            pages_succeeded: self.pages_succeeded.load(Ordering::Relaxed),
            pages_failed: self.pages_failed.load(Ordering::Relaxed),
            pages_skipped: self.pages_skipped.load(Ordering::Relaxed),
            documents_created: self.documents_created.load(Ordering::Relaxed),
            documents_indexed: self.documents_indexed.load(Ordering::Relaxed),
            bytes_processed: self.bytes_processed.load(Ordering::Relaxed),
            active_processors: self.active_processors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
struct MetricsSnapshot {
    pages_processed: u64,
    pages_succeeded: u64,
    pages_failed: u64,
    pages_skipped: u64,
    documents_created: u64,
    documents_indexed: u64,
    bytes_processed: u64,
    active_processors: u64,
}

/// AI configuration for the worker
#[derive(Clone)]
#[allow(dead_code)]
struct AiConfig {
    enable_summary: bool,
    enable_extraction: bool,
    extraction_prompt: Option<String>,
    summary_model: String,
    extraction_model: String,
    max_tokens: u32,
}

/// The main content worker
struct ContentWorker {
    consumer: KafkaConsumer,
    producer: KafkaProducer,
    parser: HtmlParser,
    storage: Option<Arc<MeilisearchStorage>>,
    ai_service: Option<Arc<AiService>>,
    ai_config: AiConfig,
    semaphore: Arc<Semaphore>,
    metrics: Arc<WorkerMetrics>,
    shutdown: Arc<AtomicBool>,
    worker_id: String,
    publish_to_kafka: bool,
    #[allow(dead_code)]
    skip_meilisearch: bool,
}

impl ContentWorker {
    /// Create a new content worker
    async fn new(args: &Args) -> anyhow::Result<Self> {
        let worker_id = args
            .worker_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());

        info!(worker_id = %worker_id, "Initializing content worker");

        // Create Kafka consumer
        let consumer = ConsumerBuilder::new(&args.brokers, &args.group_id)
            .client_id(format!("scrapix-content-{}", worker_id))
            .auto_offset_reset("earliest")
            .build()?;

        // Subscribe to raw pages topic
        consumer.subscribe(&[topic_names::PAGES_RAW])?;
        info!(
            topic = topic_names::PAGES_RAW,
            "Subscribed to raw pages topic"
        );

        // Create Kafka producer
        let producer = ProducerBuilder::new(&args.brokers)
            .client_id(format!("scrapix-content-{}-producer", worker_id))
            .compression("lz4")
            .build()?;

        // Create Meilisearch storage (optional)
        let storage = if !args.skip_meilisearch {
            let mut builder =
                MeilisearchStorageBuilder::new(&args.meilisearch_url, &args.default_index);

            if let Some(ref key) = args.meilisearch_key {
                builder = builder.api_key(key);
            }

            builder = builder.batch_size(args.batch_size);

            match builder.build().await {
                Ok(storage) => {
                    info!(
                        url = %args.meilisearch_url,
                        index = %args.default_index,
                        "Connected to Meilisearch"
                    );
                    Some(Arc::new(storage))
                }
                Err(e) => {
                    warn!(error = %e, "Failed to connect to Meilisearch, indexing disabled");
                    None
                }
            }
        } else {
            info!("Meilisearch indexing disabled");
            None
        };

        // Create HTML parser
        let parser_config = HtmlParserConfig {
            extract_content: args.extract_content,
            convert_to_markdown: args.convert_markdown,
            detect_language: args.detect_language,
            extract_schema: args.extract_schema,
            extract_og_tags: true,
            min_content_length: args.min_content_length,
        };
        let parser = HtmlParser::new(parser_config);

        // Create concurrency limiter
        let semaphore = Arc::new(Semaphore::new(args.concurrency));

        // Create AI config
        let ai_config = AiConfig {
            enable_summary: args.enable_summary,
            enable_extraction: args.enable_extraction,
            extraction_prompt: args.extraction_prompt.clone(),
            summary_model: args.summary_model.clone(),
            extraction_model: args.extraction_model.clone(),
            max_tokens: args.ai_max_tokens,
        };

        // Create AI service if API key is provided and any AI feature is enabled
        let ai_service = if (args.enable_summary || args.enable_extraction)
            && args.openai_api_key.is_some()
        {
            let api_key = args.openai_api_key.clone().unwrap();
            let ai_client_config = AiClientConfig {
                api_key,
                base_url: args.openai_api_base.clone(),
                max_concurrent_requests: args.ai_concurrency,
                ..Default::default()
            };

            match AiClient::new(ai_client_config) {
                Ok(client) => {
                    let client = Arc::new(client);
                    let mut service = AiService::minimal(client);

                    if args.enable_summary {
                        service = service.with_summarization();
                        info!(model = %args.summary_model, "AI summarization enabled");
                    }

                    if args.enable_extraction {
                        service = service.with_extraction();
                        info!(model = %args.extraction_model, "AI extraction enabled");
                    }

                    Some(Arc::new(service))
                }
                Err(e) => {
                    warn!(error = %e, "Failed to create AI client, AI features disabled");
                    None
                }
            }
        } else {
            if args.enable_summary || args.enable_extraction {
                warn!("AI features enabled but no API key provided, AI features disabled");
            }
            None
        };

        Ok(Self {
            consumer,
            producer,
            parser,
            storage,
            ai_service,
            ai_config,
            semaphore,
            metrics: Arc::new(WorkerMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            worker_id,
            publish_to_kafka: args.publish_to_kafka,
            skip_meilisearch: args.skip_meilisearch,
        })
    }

    /// Run the content worker
    async fn run(&self) -> anyhow::Result<()> {
        info!(worker_id = %self.worker_id, "Starting content worker main loop");

        // Start metrics reporter
        let metrics = self.metrics.clone();
        let shutdown = self.shutdown.clone();
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            while !shutdown.load(Ordering::Relaxed) {
                interval.tick().await;
                let snapshot = metrics.snapshot();
                info!(
                    processed = snapshot.pages_processed,
                    succeeded = snapshot.pages_succeeded,
                    failed = snapshot.pages_failed,
                    skipped = snapshot.pages_skipped,
                    docs_created = snapshot.documents_created,
                    docs_indexed = snapshot.documents_indexed,
                    bytes_mb = snapshot.bytes_processed / (1024 * 1024),
                    active = snapshot.active_processors,
                    "Worker metrics"
                );
            }
        });

        // Process messages
        let result = self.process_messages().await;

        // Flush any pending documents to Meilisearch
        if let Some(ref storage) = self.storage {
            if let Err(e) = storage.flush().await {
                warn!(error = %e, "Failed to flush pending documents");
            }
        }

        // Cleanup
        self.shutdown.store(true, Ordering::Relaxed);
        metrics_handle.abort();

        result
    }

    /// Process messages from the raw pages queue
    async fn process_messages(&self) -> anyhow::Result<()> {
        self.consumer
            .process::<RawPageMessage, _, _>(|msg, metadata| async move {
                if self.shutdown.load(Ordering::Relaxed) {
                    return Err(ScrapixError::Parse("Worker shutting down".into()));
                }

                debug!(
                    url = %msg.url,
                    job_id = %msg.job_id,
                    partition = metadata.partition,
                    offset = metadata.offset,
                    "Received raw page for processing"
                );

                // Acquire semaphore permit for concurrency limiting
                let _permit = self.semaphore.acquire().await.map_err(|e| {
                    ScrapixError::Parse(format!("Failed to acquire semaphore: {}", e))
                })?;

                self.metrics.processor_started();
                let result = self.process_page(&msg).await;
                self.metrics.processor_completed();

                if let Err(ref e) = result {
                    warn!(
                        url = %msg.url,
                        job_id = %msg.job_id,
                        error = %e,
                        "Failed to process page"
                    );
                }

                result
            })
            .await?;

        Ok(())
    }

    /// Process a single raw page
    async fn process_page(&self, msg: &RawPageMessage) -> scrapix_core::Result<()> {
        let start = Instant::now();
        let page_size = msg.html.len() as u64;

        // Skip non-HTML content
        if let Some(ref content_type) = msg.content_type {
            if !content_type.contains("text/html") && !content_type.contains("application/xhtml") {
                debug!(
                    url = %msg.url,
                    content_type = %content_type,
                    "Skipping non-HTML content"
                );
                self.metrics.record_skipped();
                return Ok(());
            }
        }

        // Convert RawPageMessage to RawPage
        let raw_page = RawPage {
            url: msg.url.clone(),
            final_url: msg.final_url.clone(),
            status: msg.status,
            headers: HashMap::new(),
            html: msg.html.clone(),
            content_type: msg.content_type.clone(),
            js_rendered: msg.js_rendered,
            fetched_at: chrono::DateTime::from_timestamp_millis(msg.fetched_at)
                .unwrap_or_else(chrono::Utc::now),
            fetch_duration_ms: msg.fetch_duration_ms,
        };

        // Parse the page
        let document = match self.parser.parse(&raw_page) {
            Ok(doc) => doc,
            Err(e) => {
                self.metrics.record_failure();

                // Send failure event
                let event = CrawlEvent::PageFailed {
                    job_id: msg.job_id.clone(),
                    url: msg.url.clone(),
                    error: format!("Parse error: {}", e),
                    retry_count: 0,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };
                self.publish_event(&msg.job_id, &event).await?;

                return Err(e);
            }
        };

        // Check if document has enough content
        let content_len = document.content.as_ref().map(|c| c.len()).unwrap_or(0);
        if content_len == 0 {
            debug!(
                url = %msg.url,
                "Skipping page with no content"
            );
            self.metrics.record_skipped();
            return Ok(());
        }

        let parse_duration = start.elapsed();

        info!(
            url = %msg.url,
            title = ?document.title,
            content_len = content_len,
            language = ?document.language,
            duration_ms = parse_duration.as_millis(),
            "Page parsed successfully"
        );

        // Apply AI enrichment if enabled
        let document = self.enrich_with_ai(document).await;

        let _process_duration = start.elapsed();
        self.metrics.record_success(page_size, 1);

        // Index document to Meilisearch
        if let Some(ref storage) = self.storage {
            if let Err(e) = storage.add_document(document.clone()).await {
                warn!(
                    url = %msg.url,
                    error = %e,
                    "Failed to index document to Meilisearch"
                );
            } else {
                self.metrics.record_indexed(1);
            }
        }

        // Publish document to Kafka topic
        if self.publish_to_kafka {
            let doc_msg = DocumentMessage::new(document.clone(), &msg.job_id, &msg.index_uid);

            self.producer
                .send(topic_names::DOCUMENTS, Some(&msg.job_id), &doc_msg)
                .await?;

            debug!(
                url = %msg.url,
                topic = topic_names::DOCUMENTS,
                "Published document to Kafka"
            );
        }

        // Send success event
        let event = CrawlEvent::DocumentIndexed {
            job_id: msg.job_id.clone(),
            url: msg.url.clone(),
            document_id: document.uid,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
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

    /// Enrich document with AI-generated content (summary, extraction)
    async fn enrich_with_ai(&self, mut document: Document) -> Document {
        let ai_service = match &self.ai_service {
            Some(s) => s,
            None => return document,
        };

        // Get content to process - prefer markdown, fallback to content
        let content = document
            .markdown
            .as_ref()
            .or(document.content.as_ref())
            .cloned()
            .unwrap_or_default();

        if content.is_empty() {
            return document;
        }

        // Truncate content if too long (keep first ~6000 tokens worth)
        let content_for_ai = if content.len() > 24000 {
            content[..24000].to_string()
        } else {
            content
        };

        // Generate AI summary if enabled
        if self.ai_config.enable_summary {
            match ai_service.tldr(&content_for_ai).await {
                Ok(summary) => {
                    debug!(
                        url = %document.url,
                        summary_len = summary.len(),
                        "Generated AI summary"
                    );
                    document.ai_summary = Some(summary);
                }
                Err(e) => {
                    warn!(
                        url = %document.url,
                        error = %e,
                        "Failed to generate AI summary"
                    );
                }
            }
        }

        // Run AI extraction if enabled and prompt is provided
        if self.ai_config.enable_extraction {
            if let Some(ref prompt) = self.ai_config.extraction_prompt {
                match ai_service.extract(&content_for_ai, prompt).await {
                    Ok(result) => {
                        debug!(
                            url = %document.url,
                            "Generated AI extraction"
                        );
                        document.ai_extraction = Some(result.data);
                    }
                    Err(e) => {
                        warn!(
                            url = %document.url,
                            error = %e,
                            "Failed to run AI extraction"
                        );
                    }
                }
            } else {
                warn!(
                    url = %document.url,
                    "AI extraction enabled but no prompt provided"
                );
            }
        }

        document
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
        meilisearch = %args.meilisearch_url,
        "Starting Scrapix content worker"
    );

    // Create and run worker
    let worker = Arc::new(ContentWorker::new(&args).await?);

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
        processed = metrics.pages_processed,
        succeeded = metrics.pages_succeeded,
        failed = metrics.pages_failed,
        skipped = metrics.pages_skipped,
        docs_created = metrics.documents_created,
        docs_indexed = metrics.documents_indexed,
        bytes_mb = metrics.bytes_processed / (1024 * 1024),
        "Final worker metrics"
    );

    result
}
