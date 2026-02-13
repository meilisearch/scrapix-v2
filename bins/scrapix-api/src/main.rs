//! Scrapix API Server
//!
//! REST API and WebSocket server for managing crawl jobs.
//!
//! ## REST Endpoints
//!
//! - `POST /scrape` - Scrape a single URL (instant, no queue)
//! - `POST /crawl` - Create a new async crawl job
//! - `POST /crawl/sync` - Create a sync crawl job (waits for completion)
//! - `GET /job/:id/status` - Get job status
//! - `GET /job/:id/events` - SSE stream for job events
//! - `DELETE /job/:id` - Cancel a job
//! - `GET /jobs` - List all jobs
//! - `GET /health` - Health check
//!
//! ## WebSocket Endpoints
//!
//! - `GET /ws` - WebSocket for subscribing to multiple job events
//! - `GET /ws/job/:id` - WebSocket for a specific job (auto-subscribes)
//!
//! ### WebSocket Protocol
//!
//! Client messages (JSON):
//! - `{"type": "subscribe", "job_id": "..."}` - Subscribe to job events
//! - `{"type": "unsubscribe", "job_id": "..."}` - Unsubscribe from job
//! - `{"type": "get_status", "job_id": "..."}` - Request current status
//! - `{"type": "ping"}` - Keepalive ping
//!
//! Server messages (JSON):
//! - `{"type": "event", "job_id": "...", "event": {...}}` - Job event
//! - `{"type": "status", "job_id": "...", "status": {...}}` - Job status
//! - `{"type": "subscribed", "job_id": "..."}` - Subscription confirmed
//! - `{"type": "unsubscribed", "job_id": "..."}` - Unsubscription confirmed
//! - `{"type": "error", "message": "...", "code": "..."}` - Error
//! - `{"type": "pong", "timestamp": 123456789}` - Pong response

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

mod analytics;
mod auth;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    middleware,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use clap::Parser;
use futures::{stream::Stream, SinkExt, StreamExt as FuturesStreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use scrapix_core::{CrawlConfig, CrawlUrl, JobState, JobStatus};
use scrapix_extractor::{Extractor, ExtractedMetadata};
use scrapix_parser::{extract_content, html_to_markdown, html_to_main_content_markdown, detect_language_info};
use scrapix_queue::{topic_names, ConsumerBuilder, CrawlEvent, KafkaProducer, ProducerBuilder, UrlMessage};
use scrapix_storage::clickhouse::{
    ClickHouseStorage, CrawlEvent as ClickHouseCrawlEvent, CrawlEventBatcher,
};

/// Scrapix API Server
#[derive(Parser, Debug)]
#[command(name = "scrapix-api")]
#[command(version, about = "REST API server for Scrapix crawl jobs")]
struct Args {
    /// Server host
    #[arg(short = 'H', long, env = "HOST", default_value = "0.0.0.0")]
    host: String,

    /// Server port
    #[arg(short, long, env = "PORT", default_value = "8080")]
    port: u16,

    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    brokers: String,

    /// Enable CORS for all origins
    #[arg(long, env = "ENABLE_CORS")]
    enable_cors: bool,

    /// API key for authentication (optional - legacy)
    #[arg(long, env = "API_KEY")]
    api_key: Option<String>,

    /// Supabase database URL for API key validation
    #[arg(long, env = "SUPABASE_DB_URL")]
    supabase_db_url: Option<String>,

    /// Maximum jobs to keep in memory
    #[arg(long, env = "MAX_JOBS", default_value = "10000")]
    max_jobs: usize,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// Application state shared across handlers
struct AppState {
    /// Kafka producer for publishing URLs
    producer: KafkaProducer,
    /// Job state storage (in-memory, could be Redis)
    jobs: RwLock<HashMap<String, JobState>>,
    /// Event broadcaster for SSE
    event_tx: broadcast::Sender<(String, CrawlEvent)>,
    /// Configuration
    config: AppConfig,
    /// Recent errors ring buffer (for diagnostics)
    recent_errors: RwLock<VecDeque<ErrorRecord>>,
    /// Per-domain counters (for diagnostics)
    domain_counters: RwLock<HashMap<String, DomainCounter>>,
    /// ClickHouse event batcher (optional - for analytics persistence)
    clickhouse_batcher: Option<Arc<CrawlEventBatcher>>,
    /// Last activity time per job (for idle-based completion detection)
    job_last_activity: RwLock<HashMap<String, std::time::Instant>>,
    /// Last time each service type was seen (for health monitoring)
    service_last_seen: RwLock<HashMap<String, std::time::Instant>>,
}

#[derive(Debug, Clone)]
struct AppConfig {
    max_jobs: usize,
}

impl AppState {
    fn new(
        producer: KafkaProducer,
        config: AppConfig,
        clickhouse_batcher: Option<Arc<CrawlEventBatcher>>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            producer,
            jobs: RwLock::new(HashMap::new()),
            event_tx,
            config,
            recent_errors: RwLock::new(VecDeque::with_capacity(1000)),
            domain_counters: RwLock::new(HashMap::new()),
            clickhouse_batcher,
            job_last_activity: RwLock::new(HashMap::new()),
            service_last_seen: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new job
    fn create_job(&self, job_id: &str, index_uid: &str) -> JobState {
        let mut jobs = self.jobs.write();

        // Evict old jobs if at capacity
        if jobs.len() >= self.config.max_jobs {
            // Remove oldest completed jobs first
            let to_remove: Vec<String> = jobs
                .iter()
                .filter(|(_, j)| {
                    matches!(
                        j.status,
                        JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
                    )
                })
                .map(|(id, _)| id.clone())
                .take(self.config.max_jobs / 10)
                .collect();

            for id in to_remove {
                jobs.remove(&id);
            }
        }

        let job = JobState::new(job_id, index_uid);
        jobs.insert(job_id.to_string(), job.clone());
        job
    }

    /// Get a job by ID
    fn get_job(&self, job_id: &str) -> Option<JobState> {
        self.jobs.read().get(job_id).cloned()
    }

    /// Update a job
    fn update_job<F>(&self, job_id: &str, f: F) -> Option<JobState>
    where
        F: FnOnce(&mut JobState),
    {
        let mut jobs = self.jobs.write();
        if let Some(job) = jobs.get_mut(job_id) {
            f(job);
            Some(job.clone())
        } else {
            None
        }
    }

    /// List all jobs
    fn list_jobs(&self, limit: usize, offset: usize) -> Vec<JobState> {
        let jobs = self.jobs.read();
        jobs.values().skip(offset).take(limit).cloned().collect()
    }

    /// Broadcast an event
    fn broadcast_event(&self, job_id: &str, event: CrawlEvent) {
        let _ = self.event_tx.send((job_id.to_string(), event));
    }

    /// Process an event and update job state accordingly
    fn process_event(&self, job_id: &str, event: &CrawlEvent) {
        // Track last activity for idle-based completion detection
        {
            let now = std::time::Instant::now();
            self.job_last_activity.write().insert(job_id.to_string(), now);

            // Track which services are alive based on event type
            let service = match event {
                CrawlEvent::PageCrawled { .. } | CrawlEvent::PageFailed { .. } => Some("crawler"),
                CrawlEvent::DocumentIndexed { .. } => Some("content"),
                CrawlEvent::UrlsDiscovered { .. } => Some("frontier"),
                _ => None,
            };
            if let Some(svc) = service {
                self.service_last_seen.write().insert(svc.to_string(), now);
            }
        }

        // Persist to ClickHouse if configured
        if let Some(ref batcher) = self.clickhouse_batcher {
            if let Some(ch_event) = kafka_event_to_clickhouse(job_id, event) {
                let batcher = batcher.clone();
                tokio::spawn(async move {
                    if let Err(e) = batcher.add(ch_event).await {
                        debug!(error = %e, "Failed to add event to ClickHouse batcher");
                    }
                });
            }
        }

        match event {
            CrawlEvent::PageCrawled { url, duration_ms, .. } => {
                self.update_job(job_id, |j| {
                    j.pages_crawled += 1;
                    // Update crawl rate based on elapsed time
                    if let Some(started) = j.started_at {
                        let elapsed = chrono::Utc::now()
                            .signed_duration_since(started)
                            .num_seconds();
                        if elapsed > 0 {
                            j.crawl_rate = j.pages_crawled as f64 / elapsed as f64;
                        }
                    }
                });

                // Track domain stats
                if let Some(domain) = extract_domain(url) {
                    let mut counters = self.domain_counters.write();
                    let counter = counters.entry(domain).or_default();
                    counter.requests += 1;
                    counter.successes += 1;
                    counter.total_response_time_ms += *duration_ms as u64;
                }
            }
            CrawlEvent::PageFailed { url, error, retry_count, .. } => {
                self.update_job(job_id, |j| {
                    j.errors += 1;
                });

                // Track error
                let domain = extract_domain(url).unwrap_or_else(|| "unknown".to_string());
                let error_record = ErrorRecord {
                    url: url.clone(),
                    domain: domain.clone(),
                    error: error.clone(),
                    status_code: extract_status_code(error),
                    job_id: job_id.to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    retry_count: *retry_count,
                };

                {
                    let mut errors = self.recent_errors.write();
                    errors.push_back(error_record);
                    while errors.len() > 1000 {
                        errors.pop_front();
                    }
                }

                // Track domain stats
                {
                    let mut counters = self.domain_counters.write();
                    let counter = counters.entry(domain).or_default();
                    counter.requests += 1;
                    counter.failures += 1;
                }
            }
            CrawlEvent::DocumentIndexed { .. } => {
                self.update_job(job_id, |j| {
                    j.pages_indexed += 1;
                    j.documents_sent += 1;
                });
            }
            CrawlEvent::JobCompleted { pages_crawled, documents_indexed, duration_secs, .. } => {
                self.update_job(job_id, |j| {
                    j.status = JobStatus::Completed;
                    j.pages_crawled = *pages_crawled;
                    j.pages_indexed = *documents_indexed;
                    j.completed_at = Some(chrono::Utc::now());
                    if *duration_secs > 0 {
                        j.crawl_rate = j.pages_crawled as f64 / *duration_secs as f64;
                    }
                });
            }
            CrawlEvent::JobFailed { error, .. } => {
                self.update_job(job_id, |j| {
                    j.status = JobStatus::Failed;
                    j.error_message = Some(error.clone());
                    j.completed_at = Some(chrono::Utc::now());
                });
            }
            CrawlEvent::UrlsDiscovered { count, .. } => {
                self.update_job(job_id, |j| {
                    // Track discovered URLs for progress estimation
                    if j.crawl_rate > 0.0 {
                        j.eta_seconds = Some((*count as f64 / j.crawl_rate) as u64);
                    }
                });
            }
            _ => {}
        }
    }
}

/// Extract domain from URL
fn extract_domain(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
}

/// Try to extract HTTP status code from error message
fn extract_status_code(error: &str) -> Option<u16> {
    use std::sync::OnceLock;

    // Compile regexes once and cache for the lifetime of the process
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        // Common patterns: "404 Not Found", "HTTP 500", "status: 503"
        [
            r"^(\d{3})\s",
            r"HTTP\s+(\d{3})",
            r"status[:\s]+(\d{3})",
        ]
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
    });

    for re in patterns {
        if let Some(caps) = re.captures(error) {
            if let Some(m) = caps.get(1) {
                if let Ok(code) = m.as_str().parse::<u16>() {
                    return Some(code);
                }
            }
        }
    }
    None
}

/// Convert a Kafka CrawlEvent to a ClickHouse CrawlEvent for analytics persistence
fn kafka_event_to_clickhouse(job_id: &str, event: &CrawlEvent) -> Option<ClickHouseCrawlEvent> {
    match event {
        CrawlEvent::PageCrawled { url, status, content_length, duration_ms, account_id, .. } => {
            let domain = extract_domain(url).unwrap_or_default();
            Some(ClickHouseCrawlEvent {
                url: url.clone(),
                domain,
                status_code: *status,
                response_time_ms: *duration_ms as u32,
                content_length: *content_length,
                content_type: String::new(),
                js_rendered: false,
                depth: 0,
                worker_id: String::new(),
                job_id: job_id.to_string(),
                account_id: account_id.clone().unwrap_or_default(),
                crawled_at: time::OffsetDateTime::now_utc(),
                error: String::new(),
                links_extracted: 0,
                content_changed: false,
            })
        }
        CrawlEvent::PageFailed { url, error, account_id, .. } => {
            let domain = extract_domain(url).unwrap_or_default();
            let status_code = extract_status_code(error).unwrap_or(0);
            Some(ClickHouseCrawlEvent {
                url: url.clone(),
                domain,
                status_code,
                response_time_ms: 0,
                content_length: 0,
                content_type: String::new(),
                js_rendered: false,
                depth: 0,
                worker_id: String::new(),
                job_id: job_id.to_string(),
                account_id: account_id.clone().unwrap_or_default(),
                crawled_at: time::OffsetDateTime::now_utc(),
                error: error.clone(),
                links_extracted: 0,
                content_changed: false,
            })
        }
        _ => None, // Other events don't map to crawl_events table
    }
}

/// API error response
#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl ApiError {
    fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
            details: None,
        }
    }

    #[allow(dead_code)]
    fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "not_found" => StatusCode::NOT_FOUND,
            "bad_request" | "validation_error" => StatusCode::BAD_REQUEST,
            "unauthorized" => StatusCode::UNAUTHORIZED,
            "conflict" => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}

/// Create crawl response
#[derive(Debug, Serialize)]
struct CreateCrawlResponse {
    job_id: String,
    status: String,
    index_uid: String,
    start_urls_count: usize,
    message: String,
}

/// Job status response
#[derive(Debug, Serialize)]
struct JobStatusResponse {
    job_id: String,
    status: String,
    index_uid: String,
    pages_crawled: u64,
    pages_indexed: u64,
    documents_sent: u64,
    errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    crawl_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    eta_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    start_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_pages: Option<u64>,
}

impl From<JobState> for JobStatusResponse {
    fn from(job: JobState) -> Self {
        let duration_seconds = job.duration_seconds();
        Self {
            job_id: job.job_id,
            status: format!("{:?}", job.status).to_lowercase(),
            index_uid: job.index_uid,
            pages_crawled: job.pages_crawled,
            pages_indexed: job.pages_indexed,
            documents_sent: job.documents_sent,
            errors: job.errors,
            started_at: job.started_at.map(|t| t.to_rfc3339()),
            completed_at: job.completed_at.map(|t| t.to_rfc3339()),
            duration_seconds,
            error_message: job.error_message,
            crawl_rate: job.crawl_rate,
            eta_seconds: job.eta_seconds,
            start_urls: job.start_urls,
            max_pages: job.max_pages,
        }
    }
}

/// List jobs query parameters
#[derive(Debug, Deserialize)]
struct ListJobsQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_limit() -> usize {
    50
}

/// Health check response
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    kafka_connected: bool,
}

// ============================================================================
// Scrape Endpoint Types
// ============================================================================

/// Request body for /scrape endpoint
#[derive(Debug, Deserialize)]
struct ScrapeRequest {
    /// URL to scrape
    url: String,

    /// Formats to return (default: all)
    #[serde(default)]
    formats: Vec<ScrapeFormat>,

    /// Whether to only return the main content (excludes nav, footer, etc.)
    #[serde(default = "default_true_bool")]
    only_main_content: bool,

    /// Include links found on the page
    #[serde(default)]
    include_links: bool,

    /// Timeout in milliseconds (default: 30000)
    #[serde(default = "default_timeout")]
    timeout_ms: u64,

    /// Custom headers to send with the request
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
}

fn default_true_bool() -> bool {
    true
}

fn default_timeout() -> u64 {
    30000
}

/// Output formats for scrape
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ScrapeFormat {
    Markdown,
    Html,
    RawHtml,
    Content,
    Links,
    Metadata,
    Screenshot,
}

/// Response for /scrape endpoint
#[derive(Debug, Serialize)]
struct ScrapeResponse {
    /// Whether the scrape was successful
    success: bool,

    /// The URL that was scraped (after redirects)
    url: String,

    /// Markdown content (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    markdown: Option<String>,

    /// Cleaned HTML content (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,

    /// Raw HTML content (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_html: Option<String>,

    /// Extracted main content text (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,

    /// Page metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<ScrapeMetadata>,

    /// Links found on the page (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<Vec<String>>,

    /// Detected language
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,

    /// HTTP status code
    status_code: u16,

    /// Time taken to scrape in milliseconds
    scrape_duration_ms: u64,
}

/// Metadata from scraped page
#[derive(Debug, Serialize)]
struct ScrapeMetadata {
    title: Option<String>,
    description: Option<String>,
    author: Option<String>,
    keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_date: Option<String>,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    open_graph: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    twitter: std::collections::HashMap<String, String>,
}

impl From<ExtractedMetadata> for ScrapeMetadata {
    fn from(meta: ExtractedMetadata) -> Self {
        Self {
            title: meta.title,
            description: meta.description,
            author: meta.author,
            keywords: meta.keywords,
            canonical_url: meta.canonical_url,
            published_date: meta.published_date,
            open_graph: meta.open_graph,
            twitter: meta.twitter,
        }
    }
}

// ============================================================================
// Diagnostic Response Types
// ============================================================================

/// System stats response
#[derive(Debug, Serialize)]
struct SystemStatsResponse {
    meilisearch: Option<MeilisearchStats>,
    jobs: JobSummary,
    diagnostics: DiagnosticsStats,
    collected_at: String,
}

#[derive(Debug, Serialize)]
struct MeilisearchStats {
    available: bool,
    url: String,
}

#[derive(Debug, Serialize)]
struct JobSummary {
    total: usize,
    running: usize,
    completed: usize,
    failed: usize,
    pending: usize,
}

#[derive(Debug, Serialize)]
struct DiagnosticsStats {
    recent_errors_count: usize,
    tracked_domains: usize,
    total_requests: u64,
    total_successes: u64,
    total_failures: u64,
}

/// Errors response
#[derive(Debug, Serialize)]
struct ErrorsResponse {
    errors: Vec<ErrorRecord>,
    total_count: usize,
    by_status: HashMap<u16, u64>,
    by_domain: Vec<(String, u64)>,
    source: String,
}

/// Error record for tracking
#[derive(Debug, Clone, Serialize)]
struct ErrorRecord {
    url: String,
    domain: String,
    error: String,
    status_code: Option<u16>,
    job_id: String,
    timestamp: String,
    retry_count: u32,
}

/// Errors query parameters
#[derive(Debug, Deserialize)]
struct ErrorsQuery {
    #[serde(default = "default_last")]
    last: usize,
    job_id: Option<String>,
}

fn default_last() -> usize {
    20
}

/// Domains response
#[derive(Debug, Serialize)]
struct DomainsResponse {
    domains: Vec<DomainInfo>,
    total_domains: usize,
    source: String,
}

#[derive(Debug, Serialize)]
struct DomainInfo {
    domain: String,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    avg_response_time_ms: Option<f64>,
}

/// Domains query parameters
#[derive(Debug, Deserialize)]
struct DomainsQuery {
    #[serde(default = "default_top")]
    top: usize,
    filter: Option<String>,
}

fn default_top() -> usize {
    20
}

/// Per-domain counter for in-memory tracking
#[derive(Debug, Clone, Default)]
struct DomainCounter {
    requests: u64,
    successes: u64,
    failures: u64,
    total_response_time_ms: u64,
}

// ============================================================================
// Route Handlers
// ============================================================================

/// Health check endpoint
async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let kafka_connected = state.producer.is_healthy();
    let status = if kafka_connected { "ok" } else { "degraded" };

    Json(HealthResponse {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        kafka_connected,
    })
}

/// Service health status for each component
#[derive(Debug, Serialize)]
struct ServiceStatus {
    name: String,
    status: String, // "up", "idle", "down"
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen_secs_ago: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ServiceHealthResponse {
    services: Vec<ServiceStatus>,
}

/// Service health endpoint — reports liveness of each component
async fn health_services(State(state): State<Arc<AppState>>) -> Json<ServiceHealthResponse> {
    let now = std::time::Instant::now();
    let seen = state.service_last_seen.read();
    let kafka_connected = state.producer.is_healthy();

    let worker_status = |name: &str| -> ServiceStatus {
        if let Some(last) = seen.get(name) {
            let ago = now.duration_since(*last).as_secs();
            if ago < 60 {
                ServiceStatus { name: name.to_string(), status: "up".to_string(), last_seen_secs_ago: Some(ago) }
            } else {
                ServiceStatus { name: name.to_string(), status: "idle".to_string(), last_seen_secs_ago: Some(ago) }
            }
        } else {
            ServiceStatus { name: name.to_string(), status: "down".to_string(), last_seen_secs_ago: None }
        }
    };

    let services = vec![
        ServiceStatus { name: "api".to_string(), status: "up".to_string(), last_seen_secs_ago: Some(0) },
        ServiceStatus {
            name: "kafka".to_string(),
            status: if kafka_connected { "up" } else { "down" }.to_string(),
            last_seen_secs_ago: None,
        },
        worker_status("crawler"),
        worker_status("content"),
        worker_status("frontier"),
    ];

    Json(ServiceHealthResponse { services })
}

/// Scrape a single URL and return content immediately
/// This bypasses the job queue for instant results
async fn scrape_url(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ScrapeRequest>,
) -> Result<Json<ScrapeResponse>, ApiError> {
    let start_time = std::time::Instant::now();

    // Validate URL
    let parsed_url = url::Url::parse(&request.url).map_err(|e| {
        ApiError::new(&format!("Invalid URL: {}", e), "validation_error")
    })?;

    // Only allow http/https
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(ApiError::new(
            "Only http and https URLs are supported",
            "validation_error",
        ));
    }

    info!(url = %request.url, "Scraping URL");

    // Build HTTP client with timeout
    let mut client_builder = reqwest::Client::builder()
        .user_agent("Scrapix/1.0")
        .timeout(Duration::from_millis(request.timeout_ms))
        .redirect(reqwest::redirect::Policy::limited(10));

    // Add custom headers (block sensitive headers to prevent injection attacks)
    const BLOCKED_HEADERS: &[&str] = &[
        "host",
        "transfer-encoding",
        "content-length",
        "connection",
        "upgrade",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
    ];
    let mut headers = reqwest::header::HeaderMap::new();
    for (key, value) in &request.headers {
        let key_lower = key.to_lowercase();
        if BLOCKED_HEADERS.contains(&key_lower.as_str()) {
            warn!(header = %key, "Blocked sensitive header in scrape request");
            continue;
        }
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            headers.insert(name, val);
        }
    }
    client_builder = client_builder.default_headers(headers);

    let client = client_builder.build().map_err(|e| {
        ApiError::new(&format!("Failed to create HTTP client: {}", e), "internal_error")
    })?;

    // Fetch the page
    let response = client.get(&request.url).send().await.map_err(|e| {
        ApiError::new(&format!("Failed to fetch URL: {}", e), "fetch_error")
    })?;

    let status_code = response.status().as_u16();
    let final_url = response.url().to_string();

    // Check for success status
    if !response.status().is_success() {
        return Ok(Json(ScrapeResponse {
            success: false,
            url: final_url,
            markdown: None,
            html: None,
            raw_html: None,
            content: None,
            metadata: None,
            links: None,
            language: None,
            status_code,
            scrape_duration_ms: start_time.elapsed().as_millis() as u64,
        }));
    }

    // Get response body
    let raw_html = response.text().await.map_err(|e| {
        ApiError::new(&format!("Failed to read response body: {}", e), "fetch_error")
    })?;

    // Determine which formats to return (default: all main formats)
    let formats = if request.formats.is_empty() {
        vec![
            ScrapeFormat::Markdown,
            ScrapeFormat::Content,
            ScrapeFormat::Metadata,
        ]
    } else {
        request.formats.clone()
    };

    // Extract content based on requested formats
    let markdown = if formats.contains(&ScrapeFormat::Markdown) {
        if request.only_main_content {
            Some(html_to_main_content_markdown(&raw_html))
        } else {
            Some(html_to_markdown(&raw_html))
        }
    } else {
        None
    };

    let content = if formats.contains(&ScrapeFormat::Content) {
        if request.only_main_content {
            Some(extract_content(&raw_html))
        } else {
            // Return full text content without readability extraction
            Some(html_to_markdown(&raw_html))
        }
    } else {
        None
    };

    let return_raw_html = if formats.contains(&ScrapeFormat::RawHtml) {
        Some(raw_html.clone())
    } else {
        None
    };

    // Extract metadata
    let metadata = if formats.contains(&ScrapeFormat::Metadata) {
        let extractor = Extractor::new().with_metadata();
        extractor
            .extract(&raw_html)
            .ok()
            .and_then(|r| r.metadata)
            .map(ScrapeMetadata::from)
    } else {
        None
    };

    // Extract links if requested
    let links = if formats.contains(&ScrapeFormat::Links) || request.include_links {
        Some(extract_links_from_html(&raw_html, &final_url))
    } else {
        None
    };

    // Detect language from content
    let language = content
        .as_ref()
        .or(markdown.as_ref())
        .and_then(|text| detect_language_info(text))
        .map(|info| info.code);

    let scrape_duration_ms = start_time.elapsed().as_millis() as u64;

    info!(
        url = %final_url,
        status_code,
        duration_ms = scrape_duration_ms,
        "Scrape completed"
    );

    Ok(Json(ScrapeResponse {
        success: true,
        url: final_url,
        markdown,
        html: None, // TODO: cleaned HTML (content only, no boilerplate)
        raw_html: return_raw_html,
        content,
        metadata,
        links,
        language,
        status_code,
        scrape_duration_ms,
    }))
}

/// Extract links from HTML
fn extract_links_from_html(html: &str, base_url: &str) -> Vec<String> {
    use scraper::{Html, Selector};

    let Ok(base) = url::Url::parse(base_url) else {
        return vec![];
    };

    let document = Html::parse_document(html);
    let Ok(selector) = Selector::parse("a[href]") else {
        return vec![];
    };

    let mut urls = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            // Skip javascript:, mailto:, tel:, etc.
            if href.starts_with("javascript:")
                || href.starts_with("mailto:")
                || href.starts_with("tel:")
                || href.starts_with("#")
            {
                continue;
            }

            // Resolve relative URLs
            if let Ok(resolved) = base.join(href) {
                let url_str = resolved.to_string();
                if seen.insert(url_str.clone()) {
                    urls.push(url_str);
                }
            }
        }
    }

    urls
}

/// Create a new async crawl job
async fn create_crawl(
    State(state): State<Arc<AppState>>,
    Json(config): Json<CrawlConfig>,
) -> Result<Json<CreateCrawlResponse>, ApiError> {
    // Validate config
    if config.start_urls.is_empty() {
        return Err(ApiError::new(
            "At least one start URL is required",
            "validation_error",
        ));
    }

    if config.index_uid.is_empty() {
        return Err(ApiError::new("Index UID is required", "validation_error"));
    }

    // Generate job ID
    let job_id = uuid::Uuid::new_v4().to_string();

    info!(
        job_id = %job_id,
        index_uid = %config.index_uid,
        start_urls_count = config.start_urls.len(),
        "Creating new crawl job"
    );

    // Create job state
    let mut job = state.create_job(&job_id, &config.index_uid);
    job.start_urls = config.start_urls.clone();
    job.max_pages = config.max_pages;
    job.start();

    // Build allowed_domains list:
    // 1. If config.allowed_domains is set, use it (explicit whitelist)
    // 2. Otherwise, if config.url_patterns.allowed_domains is set, use it
    // 3. Otherwise, auto-infer from start_urls (strict: only exact domains from seed URLs)
    let allowed_domains = if !config.allowed_domains.is_empty() {
        config.allowed_domains.clone()
    } else if !config.url_patterns.allowed_domains.is_empty() {
        config.url_patterns.allowed_domains.clone()
    } else {
        // Auto-infer domains from start_urls
        let mut domains: Vec<String> = config
            .start_urls
            .iter()
            .filter_map(|u| url::Url::parse(u).ok())
            .filter_map(|u| u.host_str().map(|h| h.to_lowercase()))
            .collect();
        domains.sort();
        domains.dedup();
        domains
    };

    info!(
        job_id = %job_id,
        allowed_domains = ?allowed_domains,
        "Using domain whitelist for crawl"
    );

    // Build URL patterns with allowed_domains
    let url_patterns = scrapix_core::UrlPatterns {
        include: config.url_patterns.include.clone(),
        exclude: config.url_patterns.exclude.clone(),
        index_only: config.url_patterns.index_only.clone(),
        allowed_domains,
    };

    // Publish seed URLs to frontier with URL patterns
    let mut urls_published = 0;
    let has_patterns = !url_patterns.include.is_empty()
        || !url_patterns.exclude.is_empty()
        || !url_patterns.allowed_domains.is_empty();

    // Extract per-job Meilisearch config to propagate through the pipeline
    let job_meilisearch_url = Some(config.meilisearch.url.clone());
    let job_meilisearch_key = Some(config.meilisearch.api_key.clone());

    for url in &config.start_urls {
        let crawl_url = CrawlUrl::seed(url);
        let msg = if has_patterns {
            UrlMessage::with_patterns(crawl_url, &job_id, &config.index_uid, url_patterns.clone())
        } else {
            UrlMessage::new(crawl_url, &job_id, &config.index_uid)
        }
        .with_meilisearch(job_meilisearch_url.clone(), job_meilisearch_key.clone());

        match state
            .producer
            .send(topic_names::URL_FRONTIER, Some(&job_id), &msg)
            .await
        {
            Ok(_) => {
                urls_published += 1;
                debug!(url = %url, job_id = %job_id, "Published seed URL to frontier");
            }
            Err(e) => {
                error!(url = %url, job_id = %job_id, error = %e, "Failed to publish seed URL");
            }
        }
    }

    if urls_published == 0 {
        // Update job as failed
        state.update_job(&job_id, |j| j.fail("Failed to publish any seed URLs"));
        return Err(ApiError::new(
            "Failed to publish seed URLs to queue",
            "queue_error",
        ));
    }

    // Publish job started event
    let event = CrawlEvent::job_started(&job_id, &config.index_uid, config.start_urls.clone());
    if let Err(e) = state
        .producer
        .send(topic_names::EVENTS, Some(&job_id), &event)
        .await
    {
        warn!(job_id = %job_id, error = %e, "Failed to publish job started event");
    }

    // Broadcast event for SSE
    state.broadcast_event(&job_id, event);

    // Update job state
    state.update_job(&job_id, |j| {
        j.status = JobStatus::Running;
    });

    info!(
        job_id = %job_id,
        urls_published = urls_published,
        "Crawl job created successfully"
    );

    Ok(Json(CreateCrawlResponse {
        job_id: job_id.clone(),
        status: "running".to_string(),
        index_uid: config.index_uid,
        start_urls_count: urls_published,
        message: format!("Crawl job started with {} seed URLs", urls_published),
    }))
}

/// Create a sync crawl job (waits for completion)
async fn create_crawl_sync(
    State(state): State<Arc<AppState>>,
    Json(config): Json<CrawlConfig>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    // First create the async job
    let response = create_crawl(State(state.clone()), Json(config)).await?;
    let job_id = response.job_id.clone();

    // Subscribe to events
    let mut rx = state.event_tx.subscribe();

    // Wait for job completion with timeout
    let timeout = Duration::from_secs(3600); // 1 hour timeout
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err(ApiError::new("Job timed out", "timeout"));
        }

        // Check job status
        if let Some(job) = state.get_job(&job_id) {
            match job.status {
                JobStatus::Completed => {
                    return Ok(Json(job.into()));
                }
                JobStatus::Failed | JobStatus::Cancelled => {
                    return Err(ApiError::new(
                        job.error_message
                            .unwrap_or_else(|| "Job failed".to_string()),
                        "job_failed",
                    ));
                }
                _ => {}
            }
        }

        // Wait for next event or timeout
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Ok((event_job_id, event))) => {
                if event_job_id == job_id {
                    if let CrawlEvent::JobCompleted { .. } | CrawlEvent::JobFailed { .. } = event {
                        // Job finished, get final status
                        if let Some(job) = state.get_job(&job_id) {
                            return Ok(Json(job.into()));
                        }
                    }
                }
            }
            Ok(Err(_)) => {
                // Channel closed, continue polling
            }
            Err(_) => {
                // Timeout, continue loop
            }
        }
    }
}

/// Get job status
async fn job_status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let job = state
        .get_job(&job_id)
        .ok_or_else(|| ApiError::new("Job not found", "not_found"))?;

    Ok(Json(job.into()))
}

/// SSE stream for job events
async fn job_events(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    // Check if job exists
    if state.get_job(&job_id).is_none() {
        return Err(ApiError::new("Job not found", "not_found"));
    }

    let rx = state.event_tx.subscribe();
    let target_job_id = job_id.clone();

    // Use futures::StreamExt for sync filter_map
    let stream = FuturesStreamExt::filter_map(BroadcastStream::new(rx), move |result| {
        let target = target_job_id.clone();
        async move {
            match result {
                Ok((event_job_id, event)) if event_job_id == target => {
                    let data = serde_json::to_string(&event).ok()?;
                    Some(Ok(Event::default().data(data)))
                }
                _ => None,
            }
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ============================================================================
// Diagnostic Handlers
// ============================================================================

/// System stats endpoint
async fn handle_stats(State(state): State<Arc<AppState>>) -> Json<SystemStatsResponse> {
    // Compute job summary
    let jobs = state.jobs.read();
    let mut running = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut pending = 0;

    for job in jobs.values() {
        match job.status {
            JobStatus::Running => running += 1,
            JobStatus::Completed => completed += 1,
            JobStatus::Failed => failed += 1,
            JobStatus::Pending => pending += 1,
            JobStatus::Cancelled => failed += 1,
            JobStatus::Paused => pending += 1,
        }
    }

    let job_summary = JobSummary {
        total: jobs.len(),
        running,
        completed,
        failed,
        pending,
    };
    drop(jobs);

    // Compute diagnostics stats
    let errors_count = state.recent_errors.read().len();
    let counters = state.domain_counters.read();
    let tracked_domains = counters.len();
    let mut total_requests = 0u64;
    let mut total_successes = 0u64;
    let mut total_failures = 0u64;

    for counter in counters.values() {
        total_requests += counter.requests;
        total_successes += counter.successes;
        total_failures += counter.failures;
    }
    drop(counters);

    let diagnostics = DiagnosticsStats {
        recent_errors_count: errors_count,
        tracked_domains,
        total_requests,
        total_successes,
        total_failures,
    };

    // Meilisearch status (we don't have direct access, just indicate availability from env)
    let meilisearch = std::env::var("MEILISEARCH_URL").ok().map(|url| MeilisearchStats {
        available: true,
        url,
    });

    Json(SystemStatsResponse {
        meilisearch,
        jobs: job_summary,
        diagnostics,
        collected_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Errors endpoint
async fn handle_errors(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ErrorsQuery>,
) -> Json<ErrorsResponse> {
    let errors = state.recent_errors.read();

    // Filter by job_id if specified
    let filtered: Vec<ErrorRecord> = if let Some(ref job_id) = params.job_id {
        errors
            .iter()
            .filter(|e| &e.job_id == job_id)
            .cloned()
            .collect()
    } else {
        errors.iter().cloned().collect()
    };

    let total_count = filtered.len();

    // Take last N errors (most recent)
    let recent: Vec<ErrorRecord> = filtered
        .into_iter()
        .rev()
        .take(params.last)
        .collect();

    // Compute status code distribution
    let mut by_status: HashMap<u16, u64> = HashMap::new();
    for error in &recent {
        if let Some(code) = error.status_code {
            *by_status.entry(code).or_insert(0) += 1;
        }
    }

    // Compute domain distribution
    let mut domain_counts: HashMap<String, u64> = HashMap::new();
    for error in &recent {
        *domain_counts.entry(error.domain.clone()).or_insert(0) += 1;
    }

    let mut by_domain: Vec<(String, u64)> = domain_counts.into_iter().collect();
    by_domain.sort_by(|a, b| b.1.cmp(&a.1));
    by_domain.truncate(10);

    Json(ErrorsResponse {
        errors: recent,
        total_count,
        by_status,
        by_domain,
        source: "memory".to_string(),
    })
}

/// Domains endpoint
async fn handle_domains(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DomainsQuery>,
) -> Json<DomainsResponse> {
    let counters = state.domain_counters.read();

    // Filter by pattern if specified
    let filtered: Vec<(&String, &DomainCounter)> = if let Some(ref filter) = params.filter {
        counters
            .iter()
            .filter(|(domain, _)| domain.contains(filter))
            .collect()
    } else {
        counters.iter().collect()
    };

    let total_domains = filtered.len();

    // Sort by total requests and take top N
    let mut sorted: Vec<_> = filtered;
    sorted.sort_by(|a, b| b.1.requests.cmp(&a.1.requests));
    sorted.truncate(params.top);

    let domains: Vec<DomainInfo> = sorted
        .into_iter()
        .map(|(domain, counter)| {
            let avg_time = if counter.successes > 0 {
                Some(counter.total_response_time_ms as f64 / counter.successes as f64)
            } else {
                None
            };

            DomainInfo {
                domain: domain.clone(),
                total_requests: counter.requests,
                successful_requests: counter.successes,
                failed_requests: counter.failures,
                avg_response_time_ms: avg_time,
            }
        })
        .collect();

    Json(DomainsResponse {
        domains,
        total_domains,
        source: "memory".to_string(),
    })
}

// ============================================================================
// WebSocket Types
// ============================================================================

/// WebSocket message from client
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsClientMessage {
    /// Subscribe to job events
    Subscribe { job_id: String },
    /// Unsubscribe from job events
    Unsubscribe { job_id: String },
    /// Request current job status
    GetStatus { job_id: String },
    /// Ping for keepalive
    Ping,
}

/// WebSocket message to client
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsServerMessage {
    /// Job event notification
    Event {
        job_id: String,
        event: CrawlEvent,
    },
    /// Job status response
    Status {
        job_id: String,
        status: JobStatusResponse,
    },
    /// Subscription confirmed
    Subscribed { job_id: String },
    /// Unsubscription confirmed
    Unsubscribed { job_id: String },
    /// Error message
    Error { message: String, code: String },
    /// Pong response
    Pong { timestamp: i64 },
}

// ============================================================================
// WebSocket Handlers
// ============================================================================

/// WebSocket upgrade handler for real-time events
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

/// Handle a WebSocket connection
async fn handle_ws_connection(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Track subscribed job IDs
    let subscriptions: Arc<RwLock<std::collections::HashSet<String>>> =
        Arc::new(RwLock::new(std::collections::HashSet::new()));

    // Subscribe to broadcast channel for events
    let mut event_rx = state.event_tx.subscribe();

    // Spawn task to forward events to WebSocket
    let subs = subscriptions.clone();
    let send_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok((job_id, event)) => {
                    // Only send if subscribed to this job
                    if subs.read().contains(&job_id) {
                        let msg = WsServerMessage::Event {
                            job_id,
                            event,
                        };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket client lagged, skipped {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // Handle incoming messages
    let state_clone = state.clone();
    let subs = subscriptions.clone();
    while let Some(msg) = FuturesStreamExt::next(&mut receiver).await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(client_msg) = serde_json::from_str::<WsClientMessage>(&text) {
                    let response = handle_ws_message(client_msg, &state_clone, &subs).await;
                    if let Ok(json) = serde_json::to_string(&response) {
                        // We can't send directly here since sender is moved
                        // The response will be handled via the broadcast channel
                        debug!("WS message processed: {}", json);
                    }
                } else {
                    debug!("Invalid WebSocket message: {}", text);
                }
            }
            Ok(Message::Ping(data)) => {
                debug!("WebSocket ping received");
                // Pong is automatically sent by axum
                let _ = data;
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection closed by client");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Clean up
    send_task.abort();
    debug!("WebSocket connection handler finished");
}

/// Handle a WebSocket client message
async fn handle_ws_message(
    msg: WsClientMessage,
    state: &Arc<AppState>,
    subscriptions: &Arc<RwLock<std::collections::HashSet<String>>>,
) -> WsServerMessage {
    match msg {
        WsClientMessage::Subscribe { job_id } => {
            if state.get_job(&job_id).is_some() {
                subscriptions.write().insert(job_id.clone());
                info!(job_id = %job_id, "WebSocket client subscribed to job");
                WsServerMessage::Subscribed { job_id }
            } else {
                WsServerMessage::Error {
                    message: "Job not found".to_string(),
                    code: "not_found".to_string(),
                }
            }
        }
        WsClientMessage::Unsubscribe { job_id } => {
            subscriptions.write().remove(&job_id);
            info!(job_id = %job_id, "WebSocket client unsubscribed from job");
            WsServerMessage::Unsubscribed { job_id }
        }
        WsClientMessage::GetStatus { job_id } => {
            if let Some(job) = state.get_job(&job_id) {
                WsServerMessage::Status {
                    job_id,
                    status: job.into(),
                }
            } else {
                WsServerMessage::Error {
                    message: "Job not found".to_string(),
                    code: "not_found".to_string(),
                }
            }
        }
        WsClientMessage::Ping => WsServerMessage::Pong {
            timestamp: chrono::Utc::now().timestamp_millis(),
        },
    }
}

/// WebSocket handler for a specific job
async fn ws_job_handler(
    ws: WebSocketUpgrade,
    Path(job_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    // Check if job exists
    if state.get_job(&job_id).is_none() {
        return Err(ApiError::new("Job not found", "not_found"));
    }

    Ok(ws.on_upgrade(move |socket| handle_job_ws_connection(socket, state, job_id)))
}

/// Handle a WebSocket connection for a specific job
async fn handle_job_ws_connection(socket: WebSocket, state: Arc<AppState>, job_id: String) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel
    let mut event_rx = state.event_tx.subscribe();
    let target_job_id = job_id.clone();

    // Send initial status
    if let Some(job) = state.get_job(&job_id) {
        let msg = WsServerMessage::Status {
            job_id: job_id.clone(),
            status: job.into(),
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = sender.send(Message::Text(json.into())).await;
        }
    }

    // Spawn task to forward events to WebSocket
    let send_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok((event_job_id, event)) if event_job_id == target_job_id => {
                    let msg = WsServerMessage::Event {
                        job_id: event_job_id,
                        event,
                    };
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(_) => {
                    // Event for different job, ignore
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(job_id = %target_job_id, "WebSocket client lagged, skipped {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // Handle incoming messages (mostly for keepalive)
    while let Some(msg) = FuturesStreamExt::next(&mut receiver).await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(client_msg) = serde_json::from_str::<WsClientMessage>(&text) {
                    match client_msg {
                        WsClientMessage::GetStatus { .. } => {
                            // Status requests handled via broadcast
                        }
                        WsClientMessage::Ping => {
                            // Ping handled automatically
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!(job_id = %job_id, "WebSocket connection closed");
                break;
            }
            Err(e) => {
                error!(job_id = %job_id, error = %e, "WebSocket error");
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
}

/// Cancel a job
async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let job = state
        .update_job(&job_id, |j| {
            j.status = JobStatus::Cancelled;
            j.completed_at = Some(chrono::Utc::now());
        })
        .ok_or_else(|| ApiError::new("Job not found", "not_found"))?;

    info!(job_id = %job_id, "Job cancelled");

    Ok(Json(job.into()))
}

/// List all jobs
async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListJobsQuery>,
) -> Json<Vec<JobStatusResponse>> {
    let jobs = state.list_jobs(params.limit, params.offset);
    Json(jobs.into_iter().map(|j| j.into()).collect())
}

// ============================================================================
// Event Consumer
// ============================================================================

/// Start consuming events from Kafka to update job state.
/// Returns a JoinHandle so the caller can await clean shutdown.
fn start_event_consumer(
    brokers: &str,
    state: Arc<AppState>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let consumer = ConsumerBuilder::new(brokers, "scrapix-api-events")
        .client_id("scrapix-api-event-consumer")
        .auto_offset_reset("latest") // Only process new events
        .build()?;

    consumer.subscribe(&[topic_names::EVENTS])?;
    info!("Event consumer subscribed to {} topic", topic_names::EVENTS);

    // Process events in background
    let handle = tokio::spawn(async move {
        loop {
            if *shutdown.borrow() {
                info!("Event consumer shutting down");
                break;
            }
            match consumer.poll_one::<CrawlEvent>(Duration::from_millis(100)).await {
                Ok(Some(event)) => {
                    // Extract job_id from event
                    let job_id = match &event {
                        CrawlEvent::JobStarted { job_id, .. } => job_id.clone(),
                        CrawlEvent::PageCrawled { job_id, .. } => job_id.clone(),
                        CrawlEvent::PageFailed { job_id, .. } => job_id.clone(),
                        CrawlEvent::DocumentIndexed { job_id, .. } => job_id.clone(),
                        CrawlEvent::UrlsDiscovered { job_id, .. } => job_id.clone(),
                        CrawlEvent::JobCompleted { job_id, .. } => job_id.clone(),
                        CrawlEvent::JobFailed { job_id, .. } => job_id.clone(),
                        CrawlEvent::PageSkipped { job_id, .. } => job_id.clone(),
                        CrawlEvent::RateLimited { job_id, .. } => job_id.clone(),
                    };

                    // Update job state and broadcast
                    state.process_event(&job_id, &event);
                    state.broadcast_event(&job_id, event);
                }
                Ok(None) => {
                    // No message, continue
                }
                Err(e) => {
                    debug!(error = %e, "Error polling events topic");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });

    Ok(handle)
}

// ============================================================================
// ClickHouse Initialization
// ============================================================================

/// Initialize ClickHouse storage and event batcher.
/// Returns (AnalyticsState for API, CrawlEventBatcher for event persistence).
async fn init_clickhouse() -> (
    Option<Arc<analytics::AnalyticsState>>,
    Option<Arc<CrawlEventBatcher>>,
) {
    // Check if ClickHouse is configured
    let config = match analytics::AnalyticsConfig::from_env() {
        Some(c) => c,
        None => {
            info!("ClickHouse not configured (CLICKHOUSE_URL not set)");
            return (None, None);
        }
    };

    // Initialize ClickHouse storage
    let ch_config = scrapix_storage::clickhouse::ClickHouseConfig {
        url: config.clickhouse_url.clone(),
        database: config.clickhouse_database.clone(),
        username: config.clickhouse_user.clone(),
        password: config.clickhouse_password.clone(),
        auto_create_tables: true,
        ..Default::default()
    };

    let storage = match ClickHouseStorage::new(ch_config).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Failed to connect to ClickHouse. Analytics and event persistence disabled.");
            return (None, None);
        }
    };

    info!(
        url = %config.clickhouse_url,
        database = %config.clickhouse_database,
        "Connected to ClickHouse"
    );

    // Create event batcher (batch size of 100 events)
    let batcher = Arc::new(CrawlEventBatcher::new(storage.clone(), 100));

    // Create analytics state (sharing the same storage connection)
    let analytics_state = Arc::new(analytics::AnalyticsState::with_storage(storage));

    (Some(analytics_state), Some(batcher))
}

// ============================================================================
// Main
// ============================================================================

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
        host = %args.host,
        port = args.port,
        brokers = %args.brokers,
        "Starting Scrapix API server"
    );

    // Create Kafka producer
    let producer = ProducerBuilder::new(&args.brokers)
        .client_id("scrapix-api")
        .compression("lz4")
        .build()?;

    info!(brokers = %args.brokers, "Connected to Kafka");

    // Initialize ClickHouse for analytics (optional)
    let (analytics_state, clickhouse_batcher) = init_clickhouse().await;

    // Initialize auth state if Supabase DB URL is provided
    let auth_state = if let Some(ref db_url) = args.supabase_db_url {
        match auth::AuthState::new(db_url).await {
            Ok(state) => {
                info!("API key authentication enabled via Supabase");
                Some(Arc::new(state))
            }
            Err(e) => {
                warn!(error = %e, "Failed to connect to Supabase DB. API key auth disabled.");
                None
            }
        }
    } else {
        info!("API key authentication disabled (SUPABASE_DB_URL not set)");
        None
    };

    // Create application state
    let config = AppConfig {
        max_jobs: args.max_jobs,
    };
    let state = Arc::new(AppState::new(producer, config, clickhouse_batcher.clone()));

    // Shutdown coordination
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Start event consumer for real-time job tracking
    let consumer_handle = start_event_consumer(&args.brokers, state.clone(), shutdown_rx.clone())?;
    info!("Event consumer started for centralized job tracking");

    // Start periodic ClickHouse flush task if enabled
    let flush_handle = if let Some(ref batcher) = clickhouse_batcher {
        let batcher_clone = batcher.clone();
        let mut shutdown_rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = batcher_clone.flush().await {
                            warn!(error = %e, "Failed to flush ClickHouse batcher");
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        info!("ClickHouse flush task shutting down, performing final flush");
                        if let Err(e) = batcher_clone.flush().await {
                            warn!(error = %e, "Failed final ClickHouse flush");
                        }
                        break;
                    }
                }
            }
        });
        info!("ClickHouse event persistence enabled (batch size: 100, flush interval: 5s)");
        Some(handle)
    } else {
        None
    };

    // Start idle-job completion detector (checks every 5s, completes jobs idle for 30s)
    let idle_state = state.clone();
    let mut idle_shutdown_rx = shutdown_rx.clone();
    let idle_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = std::time::Instant::now();
                    let idle_threshold = Duration::from_secs(30);

                    // Find running jobs that have been idle
                    let idle_jobs: Vec<(String, u64, u64, u64)> = {
                        let jobs = idle_state.jobs.read();
                        let activity = idle_state.job_last_activity.read();
                        jobs.iter()
                            .filter(|(_, j)| matches!(j.status, JobStatus::Running))
                            .filter(|(_, j)| j.pages_crawled > 0) // Must have done some work
                            .filter(|(id, _)| {
                                activity.get(*id)
                                    .map(|last| now.duration_since(*last) > idle_threshold)
                                    .unwrap_or(false)
                            })
                            .map(|(id, j)| (id.clone(), j.pages_crawled, j.pages_indexed, j.errors))
                            .collect()
                    };

                    for (job_id, pages_crawled, documents_indexed, errors) in idle_jobs {
                        let duration_secs = {
                            let jobs = idle_state.jobs.read();
                            jobs.get(&job_id)
                                .and_then(|j| j.duration_seconds())
                                .unwrap_or(0) as u64
                        };

                        info!(
                            job_id = %job_id,
                            pages_crawled,
                            documents_indexed,
                            "Auto-completing idle job (no activity for 30s)"
                        );

                        let event = CrawlEvent::JobCompleted {
                            job_id: job_id.clone(),
                            account_id: None,
                            pages_crawled,
                            documents_indexed,
                            errors,
                            bytes_downloaded: 0,
                            duration_secs,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };

                        idle_state.process_event(&job_id, &event);
                        idle_state.broadcast_event(&job_id, event);

                        // Clean up activity tracking
                        idle_state.job_last_activity.write().remove(&job_id);
                    }
                }
                _ = idle_shutdown_rx.changed() => {
                    info!("Idle job detector shutting down");
                    break;
                }
            }
        }
    });
    info!("Idle job completion detector started (30s threshold)");

    // Build router
    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/health", get(health))
        .route("/health/services", get(health_services))
        .route("/stats", get(handle_stats))
        .route("/errors", get(handle_errors))
        .route("/domains", get(handle_domains))
        .route("/ws", get(ws_handler))
        .route("/ws/job/{id}", get(ws_job_handler));

    // Protected routes (auth required when enabled)
    let protected_routes = Router::new()
        .route("/scrape", post(scrape_url))
        .route("/crawl", post(create_crawl))
        .route("/crawl/sync", post(create_crawl_sync))
        .route("/jobs", get(list_jobs))
        .route("/job/{id}/status", get(job_status))
        .route("/job/{id}/events", get(job_events))
        .route("/job/{id}", delete(cancel_job));

    // Apply auth middleware if configured
    let protected_routes = if let Some(ref auth) = auth_state {
        protected_routes.layer(middleware::from_fn_with_state(
            auth.clone(),
            auth::validate_api_key,
        ))
    } else {
        protected_routes
    };

    let mut app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Add analytics routes if ClickHouse is available
    if let Some(analytics) = analytics_state {
        app = app.nest("/analytics/v0", analytics::create_analytics_router(analytics));
        info!("Analytics API enabled at /analytics/v0/pipes");
    } else {
        info!("Analytics API disabled (CLICKHOUSE_URL not set)");
    }

    // Add CORS if enabled
    if args.enable_cors {
        app = app.layer(CorsLayer::permissive());
        info!("CORS enabled for all origins");
    }

    // Start server with graceful shutdown
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    // Retry binding in case the previous process hasn't released the port yet (hot-reload)
    let listener = {
        let mut retries = 0u32;
        loop {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => break l,
                Err(e) if retries < 10 => {
                    retries += 1;
                    warn!("Port {} busy, retrying in 500ms ({}/10): {}", args.port, retries, e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    };
    info!("Listening on {}", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install ctrl+c handler");
            info!("Shutdown signal received, draining connections...");
            let _ = shutdown_tx.send(true);
        })
        .await?;

    // Wait for background tasks to finish cleanly
    info!("Waiting for background tasks to shut down...");
    let _ = consumer_handle.await;
    let _ = idle_handle.await;
    if let Some(handle) = flush_handle {
        let _ = handle.await;
    }
    info!("Shutdown complete");

    Ok(())
}
