//! Scrapix API Server
//!
//! REST API and WebSocket server for managing crawl jobs.
//!
//! ## REST Endpoints
//!
//! - `POST /scrape` - Scrape a single URL (instant, no queue)
//! - `POST /map` - Discover all URLs on a website with titles and descriptions
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

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub mod analytics;
pub mod auth;
pub mod billing;
pub mod configs;
pub mod engines;
pub mod jobs_db;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, Path, Query, State,
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
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing::{debug, error, info, warn};

use scrapix_ai::{AiClient, AiService, FieldDefinition as AiFieldDefinition, SchemaBuilder};
use scrapix_core::{CrawlConfig, CrawlUrl, JobState, JobStatus};
use scrapix_crawler::{
    is_non_page_url, HttpFetcher, HttpFetcherBuilder, RobotsCache, RobotsConfig, SitemapParser,
};
use scrapix_extractor::{
    ContentBlock, ExtractedMetadata, ExtractedSchema, Extractor, SelectorDefinition,
    SelectorExtractor,
};
use scrapix_parser::{
    detect_language_info, extract_content, html_to_main_content_markdown,
    html_to_main_content_minihtml, html_to_markdown, html_to_minihtml,
};
use scrapix_queue::{
    topic_names, AnyConsumer, AnyProducer, ConsumerBuilder, CrawlEvent, ProducerBuilder, UrlMessage,
};
use scrapix_storage::clickhouse::{
    AiUsageBatcher, AiUsageEvent as ClickHouseAiUsageEvent, ClickHouseStorage,
    JobEvent as ClickHouseJobEvent, JobEventBatcher, RequestEvent as ClickHouseRequestEvent,
    RequestEventBatcher,
};

/// Scrapix API Server
#[derive(Parser, Debug)]
#[command(name = "scrapix-api")]
#[command(version, about = "REST API server for Scrapix crawl jobs")]
pub struct Args {
    /// Server host
    #[arg(short = 'H', long, env = "HOST", default_value = "0.0.0.0")]
    pub host: String,

    /// Server port
    #[arg(short, long, env = "PORT", default_value = "8080")]
    pub port: u16,

    /// Kafka/Redpanda broker addresses
    #[arg(short, long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    pub brokers: String,

    /// PostgreSQL database URL
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: Option<String>,

    /// JWT secret for session tokens
    #[arg(
        long,
        env = "JWT_SECRET",
        default_value = "dev-jwt-secret-change-in-production"
    )]
    pub jwt_secret: String,

    /// Maximum jobs to keep in memory
    #[arg(long, env = "MAX_JOBS", default_value = "10000")]
    pub max_jobs: usize,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,
}

/// Crawl job state: jobs, events, activity tracking
struct CrawlState {
    /// Job state storage (in-memory, could be Redis)
    jobs: RwLock<HashMap<String, JobState>>,
    /// Event broadcaster for SSE
    event_tx: broadcast::Sender<(String, CrawlEvent)>,
    /// Last activity time per job (for idle-based completion detection)
    job_last_activity: RwLock<HashMap<String, std::time::Instant>>,
    /// Job IDs with pending counter updates awaiting DB flush
    dirty_jobs: RwLock<HashSet<String>>,
}

/// Diagnostics: errors, domain stats, service health
struct DiagnosticsState {
    /// Recent errors ring buffer (for diagnostics)
    recent_errors: RwLock<VecDeque<ErrorRecord>>,
    /// Per-domain counters (for diagnostics)
    domain_counters: RwLock<HashMap<String, DomainCounter>>,
    /// Last time each service type was seen (for health monitoring)
    service_last_seen: RwLock<HashMap<String, std::time::Instant>>,
}

/// ClickHouse analytics batchers
struct AnalyticsState {
    /// Request event batcher (billing atom: 1 row per API call)
    request_batcher: Option<Arc<RequestEventBatcher>>,
    /// AI usage batcher (per-LLM-call tracking)
    #[allow(dead_code)]
    ai_usage_batcher: Option<Arc<AiUsageBatcher>>,
    /// Job event batcher (lifecycle: JobStarted/Completed/Failed)
    job_event_batcher: Option<Arc<JobEventBatcher>>,
}

/// Application state shared across handlers
struct AppState {
    /// Message bus producer for publishing URLs
    producer: AnyProducer,
    /// Configuration
    config: AppConfig,
    /// Crawl job state
    crawl: CrawlState,
    /// Diagnostics and observability
    diagnostics: DiagnosticsState,
    /// Analytics batchers
    analytics: AnalyticsState,
    /// Shared HTTP fetcher for /scrape endpoint (connection pooling, retries, DNS cache)
    fetcher: Arc<HttpFetcher>,
    /// Optional AI service for /scrape enrichment
    ai_service: Option<Arc<AiService>>,
    /// PostgreSQL connection pool (for saved configs, cron scheduling)
    db_pool: Option<sqlx::PgPool>,
}

#[derive(Debug, Clone)]
struct AppConfig {
    max_jobs: usize,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        producer: AnyProducer,
        config: AppConfig,
        request_batcher: Option<Arc<RequestEventBatcher>>,
        ai_usage_batcher: Option<Arc<AiUsageBatcher>>,
        job_event_batcher: Option<Arc<JobEventBatcher>>,
        fetcher: Arc<HttpFetcher>,
        ai_service: Option<Arc<AiService>>,
        db_pool: Option<sqlx::PgPool>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(10_000);
        Self {
            producer,
            config,
            crawl: CrawlState {
                jobs: RwLock::new(HashMap::new()),
                event_tx,
                job_last_activity: RwLock::new(HashMap::new()),
                dirty_jobs: RwLock::new(HashSet::new()),
            },
            diagnostics: DiagnosticsState {
                recent_errors: RwLock::new(VecDeque::with_capacity(1000)),
                domain_counters: RwLock::new(HashMap::new()),
                service_last_seen: RwLock::new(HashMap::new()),
            },
            analytics: AnalyticsState {
                request_batcher,
                ai_usage_batcher,
                job_event_batcher,
            },
            fetcher,
            ai_service,
            db_pool,
        }
    }

    /// Create a new job
    fn create_job(&self, job_id: &str, index_uid: &str) -> JobState {
        let mut jobs = self.crawl.jobs.write();

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
        self.crawl.jobs.read().get(job_id).cloned()
    }

    /// Update a job
    fn update_job<F>(&self, job_id: &str, f: F) -> Option<JobState>
    where
        F: FnOnce(&mut JobState),
    {
        let mut jobs = self.crawl.jobs.write();
        if let Some(job) = jobs.get_mut(job_id) {
            f(job);
            Some(job.clone())
        } else {
            None
        }
    }

    /// List all jobs
    fn list_jobs(&self, limit: usize, offset: usize) -> Vec<JobState> {
        let jobs = self.crawl.jobs.read();
        jobs.values().skip(offset).take(limit).cloned().collect()
    }

    /// Broadcast an event
    fn broadcast_event(&self, job_id: &str, event: CrawlEvent) {
        if let Err(e) = self.crawl.event_tx.send((job_id.to_string(), event)) {
            debug!("No active event subscribers, dropping event: {}", e);
        }
    }

    /// Process an event and update job state accordingly
    fn process_event(&self, job_id: &str, event: &CrawlEvent) {
        // Track last activity for idle-based completion detection
        {
            let now = std::time::Instant::now();
            self.crawl.job_last_activity
                .write()
                .insert(job_id.to_string(), now);

            // Track which services are alive based on event type
            let service = match event {
                CrawlEvent::PageCrawled { .. } | CrawlEvent::PageFailed { .. } => Some("crawler"),
                CrawlEvent::DocumentIndexed { .. } => Some("content"),
                CrawlEvent::UrlsDiscovered { .. } => Some("frontier"),
                _ => None,
            };
            if let Some(svc) = service {
                self.diagnostics.service_last_seen.write().insert(svc.to_string(), now);
            }
        }

        // Persist lifecycle events to ClickHouse job_events (JobStarted/Completed/Failed only)
        if let Some(ref batcher) = self.analytics.job_event_batcher {
            if let Some(job_event) = crawl_event_to_job_event(job_id, event) {
                let batcher = batcher.clone();
                tokio::spawn(async move {
                    if let Err(e) = batcher.add(job_event).await {
                        debug!(error = %e, "Failed to add job event to ClickHouse batcher");
                    }
                });
            }
        }

        // Persist crawl completion to request_events (1 row per crawl job at completion)
        if let Some(ref batcher) = self.analytics.request_batcher {
            if let CrawlEvent::JobCompleted {
                account_id,
                pages_crawled,
                bytes_downloaded,
                duration_secs,
                errors,
                ..
            } = event
            {
                // Look up the job to get the start URL and index_uid
                let (url, domain) = {
                    let jobs = self.crawl.jobs.read();
                    jobs.get(job_id)
                        .map(|j| {
                            let url = j.start_urls.first().cloned().unwrap_or_default();
                            let domain = extract_domain(&url).unwrap_or_default();
                            (url, domain)
                        })
                        .unwrap_or_default()
                };

                let ch_event = ClickHouseRequestEvent {
                    account_id: account_id.clone().unwrap_or_default(),
                    job_id: job_id.to_string(),
                    operation: "crawl".to_string(),
                    url,
                    domain,
                    status_code: if *errors > 0 { 0 } else { 200 },
                    duration_ms: (*duration_secs * 1000) as u32,
                    content_length: *bytes_downloaded,
                    error: String::new(),
                    js_rendered: false,
                    ai_summary: false,
                    ai_extraction: false,
                    ai_prompt_tokens: 0,
                    ai_completion_tokens: 0,
                    ai_model: String::new(),
                    urls_found: 0,
                    pages_fetched: *pages_crawled as u32,
                    timestamp: time::OffsetDateTime::now_utc(),
                };
                let batcher = batcher.clone();
                tokio::spawn(async move {
                    if let Err(e) = batcher.add(ch_event).await {
                        debug!(error = %e, "Failed to add crawl request event to ClickHouse");
                    }
                });
            }
        }

        match event {
            CrawlEvent::PageCrawled {
                url, duration_ms, ..
            } => {
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
                self.crawl.dirty_jobs.write().insert(job_id.to_string());

                // Track domain stats
                if let Some(domain) = extract_domain(url) {
                    let mut counters = self.diagnostics.domain_counters.write();
                    let counter = counters.entry(domain).or_default();
                    counter.requests += 1;
                    counter.successes += 1;
                    counter.total_response_time_ms += *duration_ms;
                }
            }
            CrawlEvent::PageFailed {
                url,
                error,
                retry_count,
                ..
            } => {
                self.update_job(job_id, |j| {
                    j.errors += 1;
                });
                self.crawl.dirty_jobs.write().insert(job_id.to_string());

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
                    let mut errors = self.diagnostics.recent_errors.write();
                    errors.push_back(error_record);
                    while errors.len() > 1000 {
                        errors.pop_front();
                    }
                }

                // Track domain stats
                {
                    let mut counters = self.diagnostics.domain_counters.write();
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
                self.crawl.dirty_jobs.write().insert(job_id.to_string());
            }
            CrawlEvent::JobCompleted {
                account_id,
                pages_crawled,
                documents_indexed,
                duration_secs,
                ..
            } => {
                let updated = self.update_job(job_id, |j| {
                    j.status = JobStatus::Completed;
                    j.pages_crawled = *pages_crawled;
                    j.pages_indexed = *documents_indexed;
                    j.completed_at = Some(chrono::Utc::now());
                    if *duration_secs > 0 {
                        j.crawl_rate = j.pages_crawled as f64 / *duration_secs as f64;
                    }
                });
                // Immediate DB write for lifecycle event
                if let (Some(ref pool), Some(snapshot)) = (&self.db_pool, updated) {
                    self.crawl.dirty_jobs.write().remove(job_id);
                    let pool = pool.clone();
                    tokio::spawn(async move { jobs_db::update_job_full(&pool, &snapshot).await });
                }

                // Deduct credits for crawled pages (fire-and-forget)
                if let (Some(ref pool), Some(ref acct_id)) = (&self.db_pool, account_id) {
                    if *pages_crawled > 0 {
                        let pool = pool.clone();
                        let acct_id = acct_id.clone();
                        let credits = *pages_crawled as i64;
                        let job_id = job_id.to_string();
                        tokio::spawn(async move {
                            match billing::check_credits_and_deduct(
                                &pool,
                                &acct_id,
                                credits,
                                "crawl",
                                &format!("Job {} ({} pages)", job_id, credits),
                            )
                            .await
                            {
                                Ok(new_balance) => {
                                    info!(
                                        account_id = %acct_id,
                                        credits_deducted = credits,
                                        new_balance,
                                        job_id = %job_id,
                                        "Crawl credits deducted"
                                    );
                                }
                                Err(e) => {
                                    error!(
                                        account_id = %acct_id,
                                        credits = credits,
                                        job_id = %job_id,
                                        error = ?e,
                                        "Failed to deduct crawl credits"
                                    );
                                }
                            }
                        });
                    }
                }
            }
            CrawlEvent::JobFailed { error, .. } => {
                // Best-effort cleanup of temp index if this was a replace_index job
                let swap_info = {
                    let jobs = self.crawl.jobs.read();
                    jobs.get(job_id).and_then(|j| {
                        if let (Some(temp), Some(url)) =
                            (&j.swap_temp_index, &j.swap_meilisearch_url)
                        {
                            Some((
                                temp.clone(),
                                url.clone(),
                                j.swap_meilisearch_api_key.clone(),
                            ))
                        } else {
                            None
                        }
                    })
                };

                if let Some((temp_index, ms_url, ms_key)) = swap_info {
                    tokio::spawn(async move {
                        if let Err(e) =
                            scrapix_storage::meilisearch::MeilisearchStorage::cleanup_temp_index(
                                &ms_url,
                                ms_key.as_deref(),
                                &temp_index,
                            )
                            .await
                        {
                            warn!(temp = %temp_index, error = %e, "Failed to cleanup temp index after job failure");
                        } else {
                            info!(temp = %temp_index, "Cleaned up temp index after job failure");
                        }
                    });
                }

                let updated = self.update_job(job_id, |j| {
                    j.status = JobStatus::Failed;
                    j.error_message = Some(error.clone());
                    j.completed_at = Some(chrono::Utc::now());
                });
                // Immediate DB write for lifecycle event
                if let (Some(ref pool), Some(snapshot)) = (&self.db_pool, updated) {
                    self.crawl.dirty_jobs.write().remove(job_id);
                    let pool = pool.clone();
                    tokio::spawn(async move { jobs_db::update_job_full(&pool, &snapshot).await });
                }
            }
            CrawlEvent::UrlsDiscovered { count, .. } => {
                self.update_job(job_id, |j| {
                    // Track discovered URLs for progress estimation
                    if j.crawl_rate > 0.0 {
                        j.eta_seconds = Some((*count as f64 / j.crawl_rate) as u64);
                    }
                });
                self.crawl.dirty_jobs.write().insert(job_id.to_string());
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
        [r"^(\d{3})\s", r"HTTP\s+(\d{3})", r"status[:\s]+(\d{3})"]
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

/// Convert a millisecond epoch timestamp to OffsetDateTime.
fn offset_datetime_from_millis(millis: i64) -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(millis / 1000)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
}

/// Convert a CrawlEvent to a ClickHouse JobEvent. Only lifecycle events are persisted.
fn crawl_event_to_job_event(job_id: &str, event: &CrawlEvent) -> Option<ClickHouseJobEvent> {
    match event {
        CrawlEvent::JobStarted {
            index_uid,
            account_id,
            start_urls,
            timestamp,
            ..
        } => Some(ClickHouseJobEvent {
            event_type: "JobStarted".to_string(),
            job_id: job_id.to_string(),
            account_id: account_id.clone().unwrap_or_default(),
            index_uid: index_uid.clone(),
            start_urls: start_urls.clone(),
            operation: "crawl".to_string(),
            timestamp: offset_datetime_from_millis(*timestamp),
            ..Default::default()
        }),
        CrawlEvent::JobCompleted {
            account_id,
            pages_crawled,
            documents_indexed,
            errors,
            bytes_downloaded,
            duration_secs,
            timestamp,
            ..
        } => Some(ClickHouseJobEvent {
            event_type: "JobCompleted".to_string(),
            job_id: job_id.to_string(),
            account_id: account_id.clone().unwrap_or_default(),
            pages_crawled: *pages_crawled,
            documents_indexed: *documents_indexed,
            errors: *errors,
            bytes_downloaded: *bytes_downloaded,
            duration_secs: *duration_secs,
            timestamp: offset_datetime_from_millis(*timestamp),
            ..Default::default()
        }),
        CrawlEvent::JobFailed {
            account_id,
            error,
            timestamp,
            ..
        } => Some(ClickHouseJobEvent {
            event_type: "JobFailed".to_string(),
            job_id: job_id.to_string(),
            account_id: account_id.clone().unwrap_or_default(),
            error: error.clone(),
            timestamp: offset_datetime_from_millis(*timestamp),
            ..Default::default()
        }),
        _ => None, // Only lifecycle events go to job_events
    }
}

/// API error response
#[derive(Debug, Serialize)]
pub(crate) struct ApiError {
    error: String,
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl ApiError {
    pub(crate) fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
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
            "insufficient_credits" => StatusCode::PAYMENT_REQUIRED,
            "spend_limit_exceeded" => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}

// ============================================================================
// Account context helpers
// ============================================================================

use crate::auth::{AuthenticatedAccount, AuthenticatedUser};

/// Resolved account context from either API key or session auth
pub(crate) struct AccountContext {
    pub account_id: String,
}

/// Extract account context from request extensions.
/// Returns `None` when auth is not configured (no DATABASE_URL), preserving backward compatibility.
async fn extract_account_context(
    db_pool: Option<&sqlx::PgPool>,
    account_ext: &Option<Extension<AuthenticatedAccount>>,
    user_ext: &Option<Extension<AuthenticatedUser>>,
) -> Option<AccountContext> {
    // API key path
    if let Some(Extension(acct)) = account_ext {
        return Some(AccountContext {
            account_id: acct.account_id.clone(),
        });
    }

    // Session path: look up account_id via DB
    if let (Some(Extension(user)), Some(pool)) = (user_ext, db_pool) {
        let row = sqlx::query(
            "SELECT a.id FROM account_members m \
             JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 LIMIT 1",
        )
        .bind(user.user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        if let Some(row) = row {
            use sqlx::Row;
            let account_id: uuid::Uuid = row.get("id");
            return Some(AccountContext {
                account_id: account_id.to_string(),
            });
        }
    }

    // No auth configured or no valid credentials
    None
}

/// Check that the authenticated account owns the given job.
/// Returns Ok(()) when: auth is disabled, job has no account_id (legacy), or account matches.
/// Returns Err(not_found) when account_id doesn't match (don't leak job existence).
fn check_job_ownership(
    job: &JobState,
    account_ctx: &Option<AccountContext>,
) -> Result<(), ApiError> {
    if let Some(ctx) = account_ctx {
        if let Some(ref job_account) = job.account_id {
            if job_account != &ctx.account_id {
                return Err(ApiError::new("Job not found", "not_found"));
            }
        }
    }
    Ok(())
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
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<serde_json::Value>,
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
            config: job.config,
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

    /// CSS selectors to remove before extraction
    #[serde(default)]
    exclude_selectors: Vec<String>,

    /// CSS selectors to keep (only extract from these)
    #[serde(default)]
    include_selectors: Vec<String>,

    /// Custom CSS selector extraction (field_name -> selector definition)
    #[serde(default)]
    extract: HashMap<String, SelectorDefinition>,

    /// AI enrichment options
    #[serde(default)]
    ai: Option<AiOptions>,
}

/// AI enrichment options for /scrape
#[derive(Debug, Deserialize)]
struct AiOptions {
    /// Generate a TL;DR summary
    #[serde(default)]
    summary: bool,

    /// Extract structured data (prompt-based or schema-based)
    #[serde(default)]
    extract: Option<AiExtractOptions>,
}

/// AI extraction options
#[derive(Debug, Deserialize)]
struct AiExtractOptions {
    /// Natural language prompt for extraction
    #[serde(default)]
    prompt: String,

    /// Schema with field definitions (alternative to prompt)
    #[serde(default)]
    schema: Option<Vec<AiFieldDef>>,
}

/// Field definition for AI schema-based extraction
#[derive(Debug, Deserialize)]
struct AiFieldDef {
    name: String,
    description: String,
    #[serde(default = "default_string_type")]
    field_type: String,
    #[serde(default)]
    required: bool,
}

fn default_string_type() -> String {
    "string".to_string()
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
    Schema,
    Blocks,
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

    /// JSON-LD and structured data (if format "schema" requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<ExtractedSchema>,

    /// Content blocks split by headings (if format "blocks" requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<Vec<ContentBlock>>,

    /// Custom selector extraction results
    #[serde(skip_serializing_if = "Option::is_none")]
    extract: Option<HashMap<String, serde_json::Value>>,

    /// AI enrichment results
    #[serde(skip_serializing_if = "Option::is_none")]
    ai: Option<AiResult>,

    /// Warning message (e.g. "AI requires OPENAI_API_KEY")
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,

    /// HTTP status code
    status_code: u16,

    /// Time taken to scrape in milliseconds
    scrape_duration_ms: u64,
}

/// AI enrichment results
#[derive(Debug, Serialize)]
struct AiResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extract: Option<serde_json::Value>,
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
    let seen = state.diagnostics.service_last_seen.read();
    let kafka_connected = state.producer.is_healthy();

    let worker_status = |name: &str| -> ServiceStatus {
        if let Some(last) = seen.get(name) {
            let ago = now.duration_since(*last).as_secs();
            if ago < 60 {
                ServiceStatus {
                    name: name.to_string(),
                    status: "up".to_string(),
                    last_seen_secs_ago: Some(ago),
                }
            } else {
                ServiceStatus {
                    name: name.to_string(),
                    status: "idle".to_string(),
                    last_seen_secs_ago: Some(ago),
                }
            }
        } else {
            ServiceStatus {
                name: name.to_string(),
                status: "down".to_string(),
                last_seen_secs_ago: None,
            }
        }
    };

    let services = vec![
        ServiceStatus {
            name: "api".to_string(),
            status: "up".to_string(),
            last_seen_secs_ago: Some(0),
        },
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

/// Preprocess HTML by applying include/exclude CSS selectors.
/// - `include_selectors`: if non-empty, only keep HTML from matching elements
/// - `exclude_selectors`: if non-empty, remove matching elements from the HTML
fn preprocess_html(
    html: &str,
    include_selectors: &[String],
    exclude_selectors: &[String],
) -> String {
    use scraper::{Html, Selector};

    if include_selectors.is_empty() && exclude_selectors.is_empty() {
        return html.to_string();
    }

    let document = Html::parse_document(html);

    // Step 1: If include_selectors are specified, collect matching elements' HTML
    let working_html = if !include_selectors.is_empty() {
        let mut parts = Vec::new();
        for sel_str in include_selectors {
            match Selector::parse(sel_str) {
                Ok(selector) => {
                    for element in document.select(&selector) {
                        parts.push(element.html());
                    }
                }
                Err(e) => {
                    warn!(selector = %sel_str, error = ?e, "Invalid include CSS selector, skipping");
                }
            }
        }
        if parts.is_empty() {
            return html.to_string();
        }
        format!("<html><body>{}</body></html>", parts.join(""))
    } else {
        html.to_string()
    };

    // Step 2: If exclude_selectors are specified, remove matching elements
    if !exclude_selectors.is_empty() {
        let mut result = working_html;
        for sel_str in exclude_selectors {
            match Selector::parse(sel_str) {
                Ok(selector) => {
                    let doc = Html::parse_document(&result);
                    for element in doc.select(&selector) {
                        let outer = element.html();
                        result = result.replacen(&outer, "", 1);
                    }
                }
                Err(e) => {
                    warn!(selector = %sel_str, error = ?e, "Invalid exclude CSS selector, skipping");
                }
            }
        }
        result
    } else {
        working_html
    }
}

/// Scrape a single URL and return content immediately
/// This bypasses the job queue for instant results
async fn scrape_url(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<ScrapeRequest>,
) -> Result<Json<ScrapeResponse>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;

    if let Some(ref ctx) = account_ctx {
        debug!(account_id = %ctx.account_id, "Scrape request from account");
    }

    // Pre-flight credit check (soft UX check; real deduction is atomic below)
    if let (Some(ref pool), Some(ref ctx)) = (&state.db_pool, &account_ctx) {
        billing::check_credits(pool, &ctx.account_id, 1).await?;
    }

    let start_time = std::time::Instant::now();

    // Validate URL
    let parsed_url = url::Url::parse(&request.url)
        .map_err(|e| ApiError::new(format!("Invalid URL: {}", e), "validation_error"))?;

    // Only allow http/https
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(ApiError::new(
            "Only http and https URLs are supported",
            "validation_error",
        ));
    }

    // Block raw IP addresses to prevent SSRF
    if matches!(parsed_url.host(), Some(url::Host::Ipv4(_)) | Some(url::Host::Ipv6(_))) {
        return Err(ApiError::new(
            "Raw IP addresses are not allowed, use a hostname instead",
            "validation_error",
        ));
    }

    info!(url = %request.url, "Scraping URL");

    // (a) Fetch using shared HttpFetcher or one-off fetcher for custom headers
    let crawl_url = CrawlUrl::seed(&request.url);

    let raw_page = if request.headers.is_empty() {
        // Use the shared fetcher (connection pooling, DNS cache, retries)
        state
            .fetcher
            .fetch(&crawl_url)
            .await
            .map_err(|e| ApiError::new(format!("Failed to fetch URL: {}", e), "fetch_error"))?
    } else {
        // Build a one-off fetcher with custom headers
        let robots_config = RobotsConfig {
            respect_robots: false,
            ..Default::default()
        };
        let robots_cache = Arc::new(RobotsCache::new(robots_config).map_err(|e| {
            ApiError::new(
                format!("Failed to create robots cache: {}", e),
                "internal_error",
            )
        })?);

        let mut builder =
            HttpFetcherBuilder::new().timeout(Duration::from_millis(request.timeout_ms));

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
        for (key, value) in &request.headers {
            let key_lower = key.to_lowercase();
            if BLOCKED_HEADERS.contains(&key_lower.as_str()) {
                warn!(header = %key, "Blocked sensitive header in scrape request");
                continue;
            }
            builder = builder.header(key, value);
        }

        let fetcher = builder.build(robots_cache).map_err(|e| {
            ApiError::new(
                format!("Failed to create HTTP client: {}", e),
                "internal_error",
            )
        })?;

        fetcher
            .fetch(&crawl_url)
            .await
            .map_err(|e| ApiError::new(format!("Failed to fetch URL: {}", e), "fetch_error"))?
    };

    let status_code = raw_page.status;
    let final_url = raw_page.final_url.clone();

    // Check for success status
    if !(200..300).contains(&status_code) {
        // Track failed scrape in ClickHouse request_events
        if let Some(ref batcher) = state.analytics.request_batcher {
            let account_id = account_ctx
                .as_ref()
                .map(|c| c.account_id.clone())
                .unwrap_or_default();
            log_scrape_request(
                batcher,
                &final_url,
                status_code,
                start_time.elapsed().as_millis() as u64,
                0,
                account_id,
                format!("HTTP {}", status_code),
                false,
                false,
            );
        }

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
            schema: None,
            blocks: None,
            extract: None,
            ai: None,
            warning: None,
            status_code,
            scrape_duration_ms: start_time.elapsed().as_millis() as u64,
        }));
    }

    let original_html = raw_page.html;
    let original_html_len = original_html.len() as u64;

    // (b) Preprocess HTML with include/exclude selectors
    let processed_html = preprocess_html(
        &original_html,
        &request.include_selectors,
        &request.exclude_selectors,
    );

    // Determine which formats to return (default: markdown + content + metadata)
    let formats = if request.formats.is_empty() {
        vec![
            ScrapeFormat::Markdown,
            ScrapeFormat::Content,
            ScrapeFormat::Metadata,
        ]
    } else {
        request.formats.clone()
    };

    // (c) Run extractor for metadata, schema, blocks, and custom selectors
    let needs_extraction = formats.contains(&ScrapeFormat::Metadata)
        || formats.contains(&ScrapeFormat::Schema)
        || formats.contains(&ScrapeFormat::Blocks)
        || !request.extract.is_empty();

    let extraction_result = if needs_extraction {
        let mut extractor = Extractor::new();

        if formats.contains(&ScrapeFormat::Metadata) {
            extractor = extractor.with_metadata();
        }
        if formats.contains(&ScrapeFormat::Schema) {
            extractor = extractor.with_schema();
        }
        if formats.contains(&ScrapeFormat::Blocks) {
            extractor = extractor.with_blocks();
        }
        if !request.extract.is_empty() {
            let sel_extractor = SelectorExtractor::with_definitions(request.extract.clone());
            extractor = extractor.with_selectors(sel_extractor);
        }

        extractor.extract(&processed_html).ok()
    } else {
        None
    };

    // Pull out extraction results
    let metadata = extraction_result
        .as_ref()
        .and_then(|r| r.metadata.clone())
        .map(ScrapeMetadata::from);

    let schema = extraction_result.as_ref().and_then(|r| r.schema.clone());

    let blocks = extraction_result
        .as_ref()
        .and_then(|r| r.blocks.clone())
        .map(|b| b.blocks);

    let custom_extract = extraction_result
        .as_ref()
        .and_then(|r| r.custom.clone())
        .map(|c| c.values);

    // (d) Run parser functions on processed HTML
    let markdown = if formats.contains(&ScrapeFormat::Markdown) {
        if request.only_main_content {
            Some(html_to_main_content_markdown(&processed_html))
        } else {
            Some(html_to_markdown(&processed_html))
        }
    } else {
        None
    };

    let content = if formats.contains(&ScrapeFormat::Content) {
        if request.only_main_content {
            Some(extract_content(&processed_html))
        } else {
            Some(html_to_markdown(&processed_html))
        }
    } else {
        None
    };

    let html_output = if formats.contains(&ScrapeFormat::Html) {
        if request.only_main_content {
            Some(html_to_main_content_minihtml(&processed_html))
        } else {
            Some(html_to_minihtml(&processed_html))
        }
    } else {
        None
    };

    let return_raw_html = if formats.contains(&ScrapeFormat::RawHtml) {
        Some(original_html)
    } else {
        None
    };

    let links = if formats.contains(&ScrapeFormat::Links) || request.include_links {
        Some(extract_links_from_html(&processed_html, &final_url))
    } else {
        None
    };

    // Detect language from content
    let language = content
        .as_ref()
        .or(markdown.as_ref())
        .and_then(|text| detect_language_info(text))
        .map(|info| info.code);

    // (e) AI enrichment (optional)
    let mut ai_result = None;
    let mut warning = None;

    if let Some(ref ai_opts) = request.ai {
        if let Some(ref ai_service) = state.ai_service {
            // Use content or markdown as input text for AI
            let ai_text = content.as_deref().or(markdown.as_deref()).unwrap_or("");

            if !ai_text.is_empty() {
                // Run AI operations concurrently
                let summary_fut = async {
                    if ai_opts.summary {
                        ai_service.tldr(ai_text).await.ok()
                    } else {
                        None
                    }
                };

                let extract_fut = async {
                    if let Some(ref extract_opts) = ai_opts.extract {
                        if let Some(ref schema_fields) = extract_opts.schema {
                            // Schema-based extraction
                            let mut builder = SchemaBuilder::new();
                            for field in schema_fields {
                                builder = builder.field(AiFieldDefinition {
                                    name: field.name.clone(),
                                    description: field.description.clone(),
                                    field_type: field.field_type.clone(),
                                    required: field.required,
                                    default: None,
                                    example: None,
                                });
                            }
                            let schema = builder.build();
                            ai_service
                                .extract_schema(ai_text, &schema)
                                .await
                                .ok()
                                .map(|r| r.data)
                        } else if !extract_opts.prompt.is_empty() {
                            // Prompt-based extraction
                            ai_service
                                .extract(ai_text, &extract_opts.prompt)
                                .await
                                .ok()
                                .map(|r| r.data)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                let (ai_summary, ai_extract) = tokio::join!(summary_fut, extract_fut);

                if ai_summary.is_some() || ai_extract.is_some() {
                    ai_result = Some(AiResult {
                        summary: ai_summary,
                        extract: ai_extract,
                    });
                }
            }
        } else {
            warning = Some("AI features require a provider API key (set AI_PROVIDER and corresponding key env var)".to_string());
        }
    }

    let scrape_duration_ms = start_time.elapsed().as_millis() as u64;

    // Track successful scrape in ClickHouse request_events
    let has_ai_summary = request.ai.as_ref().is_some_and(|ai| ai.summary);
    let has_ai_extraction = request.ai.as_ref().is_some_and(|ai| ai.extract.is_some());
    if let Some(ref batcher) = state.analytics.request_batcher {
        let account_id = account_ctx
            .as_ref()
            .map(|c| c.account_id.clone())
            .unwrap_or_default();
        log_scrape_request(
            batcher,
            &final_url,
            status_code,
            scrape_duration_ms,
            original_html_len,
            account_id,
            String::new(),
            has_ai_summary,
            has_ai_extraction,
        );
    }

    // Deduct 1 credit for successful scrape (atomic)
    if let (Some(ref pool), Some(ref ctx)) = (&state.db_pool, &account_ctx) {
        if let Err(e) = billing::check_credits_and_deduct(
            pool,
            &ctx.account_id,
            1,
            "scrape",
            &final_url,
        )
        .await
        {
            warn!(account_id = %ctx.account_id, error = ?e, "Failed to deduct credit for scrape");
        }
    }

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
        html: html_output,
        raw_html: return_raw_html,
        content,
        metadata,
        links,
        language,
        schema,
        blocks,
        extract: custom_extract,
        ai: ai_result,
        warning,
        status_code,
        scrape_duration_ms,
    }))
}

/// Log a scrape request to ClickHouse request_events (fire-and-forget).
fn log_scrape_request(
    batcher: &Arc<RequestEventBatcher>,
    url: &str,
    status_code: u16,
    duration_ms: u64,
    content_length: u64,
    account_id: String,
    error: String,
    ai_summary: bool,
    ai_extraction: bool,
) {
    let domain = extract_domain(url).unwrap_or_default();
    let event = ClickHouseRequestEvent {
        account_id,
        job_id: String::new(),
        operation: "scrape".to_string(),
        url: url.to_string(),
        domain,
        status_code,
        duration_ms: duration_ms as u32,
        content_length,
        error,
        js_rendered: false,
        ai_summary,
        ai_extraction,
        ai_prompt_tokens: 0,
        ai_completion_tokens: 0,
        ai_model: String::new(),
        urls_found: 0,
        pages_fetched: 1,
        timestamp: time::OffsetDateTime::now_utc(),
    };
    let batcher = batcher.clone();
    tokio::spawn(async move {
        if let Err(e) = batcher.add(event).await {
            debug!(error = %e, "Failed to add scrape request event to ClickHouse");
        }
    });
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

/// Core crawl creation logic, reusable from handler, trigger, and cron scheduler
pub(crate) async fn do_create_crawl(
    state: &Arc<AppState>,
    config: CrawlConfig,
    account_ctx: Option<&AccountContext>,
) -> Result<CreateCrawlResponse, ApiError> {
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

    // Pre-flight credit check (1 credit minimum to start a crawl)
    if let (Some(ref pool), Some(ctx)) = (&state.db_pool, account_ctx) {
        billing::check_credits(pool, &ctx.account_id, 1).await?;
    }

    // Generate job ID
    let job_id = uuid::Uuid::new_v4().to_string();

    // If replace_index is enabled, workers write to a temp index; we swap on completion
    let target_index_uid = config.index_uid.clone();
    let pipeline_index_uid = if config.replace_index {
        format!("{}_tmp_{}", config.index_uid, &job_id[..8])
    } else {
        config.index_uid.clone()
    };

    info!(
        job_id = %job_id,
        index_uid = %target_index_uid,
        pipeline_index_uid = %pipeline_index_uid,
        replace_index = config.replace_index,
        start_urls_count = config.start_urls.len(),
        "Creating new crawl job"
    );

    // Create job state (tracks the target index_uid, not the temp one)
    let mut job = if let Some(ctx) = account_ctx {
        let j = JobState::with_account(&job_id, &target_index_uid, &ctx.account_id);
        let mut jobs = state.crawl.jobs.write();
        // Evict old jobs if at capacity
        if jobs.len() >= state.config.max_jobs {
            let to_remove: Vec<String> = jobs
                .iter()
                .filter(|(_, j)| {
                    matches!(
                        j.status,
                        JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
                    )
                })
                .map(|(id, _)| id.clone())
                .take(state.config.max_jobs / 10)
                .collect();
            for id in to_remove {
                jobs.remove(&id);
            }
        }
        jobs.insert(job_id.clone(), j.clone());
        j
    } else {
        state.create_job(&job_id, &target_index_uid)
    };
    job.start_urls = config.start_urls.clone();
    job.max_pages = config.max_pages;
    // Redact sensitive fields before persisting config to database
    job.config = serde_json::to_value(&config).ok().map(|mut v| {
        if let Some(ms) = v.get_mut("meilisearch") {
            if let Some(obj) = ms.as_object_mut() {
                obj.insert("api_key".to_string(), serde_json::Value::String("***".to_string()));
            }
        }
        v
    });
    if config.replace_index {
        job.swap_temp_index = Some(pipeline_index_uid.clone());
        job.swap_meilisearch_url = Some(config.meilisearch.url.clone());
        job.swap_meilisearch_api_key = Some(config.meilisearch.api_key.clone());
    }
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
        // Use pipeline_index_uid (temp index if replace_index, otherwise target)
        let msg = if has_patterns {
            UrlMessage::with_patterns(
                crawl_url,
                &job_id,
                &pipeline_index_uid,
                url_patterns.clone(),
            )
        } else {
            UrlMessage::new(crawl_url, &job_id, &pipeline_index_uid)
        }
        .with_meilisearch(job_meilisearch_url.clone(), job_meilisearch_key.clone())
        .with_features(Some(config.features.clone()))
        .with_limits(config.max_depth, config.max_pages);

        // Attach account_id to message for billing attribution
        let msg = if let Some(ctx) = account_ctx {
            msg.account(&ctx.account_id)
        } else {
            msg
        };

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
    let event = if let Some(ctx) = account_ctx {
        CrawlEvent::job_started_with_account(
            &job_id,
            &target_index_uid,
            &ctx.account_id,
            config.start_urls.clone(),
        )
    } else {
        CrawlEvent::job_started(&job_id, &target_index_uid, config.start_urls.clone())
    };
    if let Err(e) = state
        .producer
        .send(topic_names::EVENTS, Some(&job_id), &event)
        .await
    {
        warn!(job_id = %job_id, error = %e, "Failed to publish job started event");
    }

    // Broadcast event for SSE
    state.broadcast_event(&job_id, event);

    // Update job state (write back config, start_urls, max_pages, swap metadata)
    let replace_index = config.replace_index;
    let swap_temp = if replace_index {
        Some(pipeline_index_uid.clone())
    } else {
        None
    };
    let swap_url = if replace_index {
        Some(config.meilisearch.url.clone())
    } else {
        None
    };
    let swap_key = if replace_index {
        Some(config.meilisearch.api_key.clone())
    } else {
        None
    };
    let snapshot = state.update_job(&job_id, |j| {
        j.status = JobStatus::Running;
        j.start_urls = config.start_urls.clone();
        j.max_pages = config.max_pages;
        // Redact sensitive fields before persisting config to database
        j.config = serde_json::to_value(&config).ok().map(|mut v| {
            if let Some(ms) = v.get_mut("meilisearch") {
                if let Some(obj) = ms.as_object_mut() {
                    obj.insert("api_key".to_string(), serde_json::Value::String("***".to_string()));
                }
            }
            v
        });
        j.started_at = Some(chrono::Utc::now());
        j.swap_temp_index = swap_temp;
        j.swap_meilisearch_url = swap_url;
        j.swap_meilisearch_api_key = swap_key;
    });

    // Persist new job to Postgres
    if let (Some(ref pool), Some(snapshot)) = (&state.db_pool, snapshot) {
        let pool = pool.clone();
        tokio::spawn(async move { jobs_db::insert_job(&pool, &snapshot).await });
    }

    info!(
        job_id = %job_id,
        urls_published = urls_published,
        "Crawl job created successfully"
    );

    Ok(CreateCrawlResponse {
        job_id: job_id.clone(),
        status: "running".to_string(),
        index_uid: target_index_uid,
        start_urls_count: urls_published,
        message: format!("Crawl job started with {} seed URLs", urls_published),
    })
}

// ============================================================================
// Map endpoint - discover URLs on a website
// ============================================================================

/// Request body for POST /map
#[derive(Debug, Deserialize)]
struct MapRequest {
    /// Website URL to map
    url: String,

    /// Maximum number of links to return (default: 5000)
    #[serde(default = "default_map_limit")]
    limit: usize,

    /// How many levels deep to follow links beyond sitemap (default: 0, max: 5)
    #[serde(default)]
    depth: u32,

    /// Filter results to URLs matching this search term
    #[serde(default)]
    search: Option<String>,

    /// Whether to use sitemaps for discovery (default: true)
    #[serde(default = "default_true_bool")]
    sitemap: bool,

    /// Fetch <title> from each page's HTML head (default: true)
    #[serde(default = "default_true_bool")]
    get_title: bool,

    /// Fetch <meta description> from each page's HTML head (default: true)
    #[serde(default = "default_true_bool")]
    get_description: bool,

    /// Include lastmod from sitemap data (default: true)
    #[serde(default = "default_true_bool")]
    get_lastmod: bool,

    /// Include priority from sitemap data (default: true)
    #[serde(default = "default_true_bool")]
    get_priority: bool,

    /// Include changefreq from sitemap data (default: true)
    #[serde(default = "default_true_bool")]
    get_changefreq: bool,
}

fn default_map_limit() -> usize {
    5000
}

/// A discovered link with optional metadata
#[derive(Debug, Clone, Serialize)]
struct MapLink {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lastmod: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    changefreq: Option<String>,
}

/// Response for POST /map
#[derive(Debug, Serialize)]
struct MapResponse {
    success: bool,
    links: Vec<MapLink>,
    /// Total number of links discovered
    total: usize,
    /// Time taken in milliseconds
    duration_ms: u64,
}

/// Result from fetching a single page during mapping
struct MapFetchResult {
    url: String,
    title: Option<String>,
    description: Option<String>,
    /// Newly discovered child links (url, anchor_text)
    child_links: Vec<(String, Option<String>)>,
}

/// Extract links from an HTML document, resolving relative URLs against a base.
/// Only returns same-domain http(s) links.
fn extract_page_links(html: &str, base_url: &url::Url) -> Vec<(String, Option<String>)> {
    use std::sync::OnceLock;
    static LINK_SELECTOR: OnceLock<scraper::Selector> = OnceLock::new();

    let document = scraper::Html::parse_document(html);
    let link_selector =
        LINK_SELECTOR.get_or_init(|| scraper::Selector::parse("a[href]").expect("valid selector"));
    let base_domain = base_url.host_str().unwrap_or("");

    let mut links = Vec::new();
    for element in document.select(&link_selector) {
        if let Some(href) = element.value().attr("href") {
            let resolved = base_url.join(href).ok();
            if let Some(resolved_url) = resolved {
                // Only same-domain http(s) links
                if matches!(resolved_url.scheme(), "http" | "https")
                    && resolved_url.host_str().unwrap_or("") == base_domain
                {
                    // Strip fragment
                    let mut clean = resolved_url;
                    clean.set_fragment(None);
                    let url_str = clean.to_string();

                    let anchor_text = element.text().collect::<String>();
                    let anchor = if anchor_text.trim().is_empty() {
                        None
                    } else {
                        Some(anchor_text.trim().to_string())
                    };
                    links.push((url_str, anchor));
                }
            }
        }
    }
    links
}

/// Fetch a single URL and extract title, description, and child links.
async fn map_fetch_page(
    fetcher: Arc<HttpFetcher>,
    url: String,
    base_url: url::Url,
) -> Option<MapFetchResult> {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE_TITLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
    static RE_DESC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<meta[^>]+name\s*=\s*["']description["'][^>]+content\s*=\s*["']([^"']*)["']"#).unwrap()
    });
    static RE_DESC_ALT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<meta[^>]+content\s*=\s*["']([^"']*)["'][^>]+name\s*=\s*["']description["']"#).unwrap()
    });

    let crawl_url = CrawlUrl::seed(&url);
    let page = match tokio::time::timeout(Duration::from_secs(10), fetcher.fetch(&crawl_url)).await
    {
        Ok(Ok(page)) if (200..300).contains(&page.status) => page,
        _ => return None,
    };

    // Extract title and description from the head via regex (avoids a full DOM parse)
    let head_end = page.html.find("</head>").unwrap_or(8192.min(page.html.len()));
    let head = &page.html[..head_end];

    let title = RE_TITLE
        .captures(head)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|t| !t.is_empty());

    let description = RE_DESC
        .captures(head)
        .or_else(|| RE_DESC_ALT.captures(head))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|d| !d.is_empty());

    let child_links = extract_page_links(&page.html, &base_url);

    Some(MapFetchResult {
        url,
        title,
        description,
        child_links,
    })
}

/// Map a website: sitemap-first discovery with optional BFS deep crawl
///
/// Discovers URLs via:
/// 1. Sitemap parsing (robots.txt → sitemap.xml → sitemap indexes)
/// 2. Optional BFS link crawling when `depth > 0`
///
/// Each URL can be enriched with title, description (from HTML head) and
/// lastmod, priority, changefreq (from sitemap data) depending on `get_*` flags.
async fn map_url(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<MapRequest>,
) -> Result<Json<MapResponse>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;

    // Pre-flight credit check
    if let (Some(ref pool), Some(ref ctx)) = (&state.db_pool, &account_ctx) {
        billing::check_credits(pool, &ctx.account_id, 1).await?;
    }

    let start_time = std::time::Instant::now();

    // Validate URL
    let parsed_url = url::Url::parse(&request.url)
        .map_err(|e| ApiError::new(format!("Invalid URL: {}", e), "validation_error"))?;

    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(ApiError::new(
            "Only http and https URLs are supported",
            "validation_error",
        ));
    }

    // Block raw IP addresses to prevent SSRF
    if matches!(parsed_url.host(), Some(url::Host::Ipv4(_)) | Some(url::Host::Ipv6(_))) {
        return Err(ApiError::new(
            "Raw IP addresses are not allowed, use a hostname instead",
            "validation_error",
        ));
    }

    let limit = request.limit.min(10_000);
    let max_depth = request.depth.min(5);
    let needs_html_fetch = request.get_title || request.get_description;

    info!(url = %request.url, limit, depth = max_depth, "Mapping website URLs");

    use futures::stream::FuturesUnordered;

    // Track visited URLs and collected results
    let mut visited = HashSet::new();
    let mut results: Vec<MapLink> = Vec::new();

    // Map from URL → sitemap metadata for enrichment
    let mut sitemap_meta: HashMap<String, (Option<String>, Option<f32>, Option<String>)> =
        HashMap::new();

    let base_url = parsed_url.clone();
    let base_domain = base_url.host_str().unwrap_or("").to_string();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(50));
    let fetcher = state.fetcher.clone();

    // Total timeout
    let map_deadline = std::time::Instant::now() + Duration::from_secs(60);

    // ── Step 1: Sitemap discovery (blocking, primary source) ──────────────

    if request.sitemap {
        let sitemap_parser = SitemapParser::with_defaults();
        match sitemap_parser.discover_all_urls(&request.url).await {
            Ok(sitemap_urls) => {
                debug!(count = sitemap_urls.len(), "Discovered URLs from sitemaps");
                for su in sitemap_urls {
                    if visited.len() >= limit {
                        break;
                    }
                    // Filter non-page URLs
                    if is_non_page_url(&su.loc) {
                        continue;
                    }
                    // Only same-domain URLs
                    if let Ok(su_parsed) = url::Url::parse(&su.loc) {
                        if su_parsed.host_str().unwrap_or("") != base_domain {
                            continue;
                        }
                    } else {
                        continue;
                    }
                    if visited.insert(su.loc.clone()) {
                        // Store sitemap metadata for later enrichment
                        let lastmod = if request.get_lastmod {
                            su.lastmod.map(|dt| dt.to_rfc3339())
                        } else {
                            None
                        };
                        let priority = if request.get_priority {
                            su.priority
                        } else {
                            None
                        };
                        let changefreq = if request.get_changefreq {
                            su.changefreq.map(|cf| format!("{:?}", cf).to_lowercase())
                        } else {
                            None
                        };
                        sitemap_meta
                            .insert(su.loc.clone(), (lastmod.clone(), priority, changefreq.clone()));
                        results.push(MapLink {
                            url: su.loc,
                            title: None,
                            description: None,
                            lastmod,
                            priority,
                            changefreq,
                        });
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Sitemap discovery failed, falling back to BFS only");
            }
        }
    }

    // If no sitemap results (or sitemap disabled), seed with the input URL
    if results.is_empty() {
        visited.insert(request.url.clone());
        results.push(MapLink {
            url: request.url.clone(),
            title: None,
            description: None,
            lastmod: None,
            priority: None,
            changefreq: None,
        });
    }

    // ── Step 2: Enrich sitemap URLs with HTML metadata (title/description) ──

    if needs_html_fetch && !results.is_empty() {
        let urls_to_fetch: Vec<String> = results
            .iter()
            .take(limit)
            .map(|r| r.url.clone())
            .collect();

        debug!(count = urls_to_fetch.len(), "Fetching HTML metadata for sitemap URLs");

        let get_title = request.get_title;
        let get_description = request.get_description;

        let mut in_flight: FuturesUnordered<_> = urls_to_fetch
            .into_iter()
            .map(|url| {
                let semaphore = semaphore.clone();
                let fetcher = fetcher.clone();
                let base_url = base_url.clone();
                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.ok()?;
                    map_fetch_page(fetcher, url, base_url).await
                })
            })
            .collect();

        // Build a lookup from fetch results
        let mut meta_map: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
        while let Some(task_result) = in_flight.next().await {
            if std::time::Instant::now() > map_deadline {
                warn!("Map metadata fetch timed out");
                break;
            }
            if let Ok(Some(fetch_result)) = task_result {
                let title = if get_title { fetch_result.title } else { None };
                let desc = if get_description {
                    fetch_result.description
                } else {
                    None
                };
                meta_map.insert(fetch_result.url, (title, desc));
            }
        }

        // Merge metadata into results
        for link in &mut results {
            if let Some((title, description)) = meta_map.remove(&link.url) {
                link.title = title;
                link.description = description;
            }
        }
    }

    // ── Step 3: BFS deep crawl (only if depth > 0) ───────────────────────

    if max_depth > 0 {
        // Build frontier from all currently known URLs
        let mut frontier: Vec<String> = results.iter().map(|r| r.url.clone()).collect();

        for current_depth in 1..=max_depth {
            if frontier.is_empty() || results.len() >= limit {
                break;
            }
            if std::time::Instant::now() > map_deadline {
                warn!(url = %request.url, "Map operation timed out during BFS");
                break;
            }

            let budget = limit.saturating_sub(results.len());
            frontier.truncate(budget);

            debug!(
                depth = current_depth,
                frontier_size = frontier.len(),
                "BFS depth level"
            );

            // Fetch all frontier pages to extract child links
            let mut in_flight: FuturesUnordered<_> = frontier
                .drain(..)
                .map(|url| {
                    let semaphore = semaphore.clone();
                    let fetcher = fetcher.clone();
                    let base_url = base_url.clone();
                    tokio::spawn(async move {
                        let _permit = semaphore.acquire().await.ok()?;
                        map_fetch_page(fetcher, url, base_url).await
                    })
                })
                .collect();

            let mut next_frontier: Vec<String> = Vec::new();

            while let Some(task_result) = in_flight.next().await {
                if let Ok(Some(fetch_result)) = task_result {
                    // Extract child links for next BFS level
                    for (child_url, _anchor) in fetch_result.child_links {
                        if is_non_page_url(&child_url) {
                            continue;
                        }
                        if visited.len() < limit && visited.insert(child_url.clone()) {
                            let title = if request.get_title {
                                None // will be fetched if we crawl this URL
                            } else {
                                None
                            };
                            let description = if request.get_description {
                                None
                            } else {
                                None
                            };
                            results.push(MapLink {
                                url: child_url.clone(),
                                title,
                                description,
                                lastmod: None,
                                priority: None,
                                changefreq: None,
                            });
                            next_frontier.push(child_url);
                        }
                    }
                }

                if results.len() >= limit {
                    break;
                }
            }

            // Enrich newly discovered URLs with HTML metadata
            if needs_html_fetch && !next_frontier.is_empty() {
                let get_title = request.get_title;
                let get_description = request.get_description;

                let mut enrich_flight: FuturesUnordered<_> = next_frontier
                    .iter()
                    .cloned()
                    .map(|url| {
                        let semaphore = semaphore.clone();
                        let fetcher = fetcher.clone();
                        let base_url = base_url.clone();
                        tokio::spawn(async move {
                            let _permit = semaphore.acquire().await.ok()?;
                            map_fetch_page(fetcher, url, base_url).await
                        })
                    })
                    .collect();

                let mut meta_map: HashMap<String, (Option<String>, Option<String>)> =
                    HashMap::new();
                while let Some(task_result) = enrich_flight.next().await {
                    if std::time::Instant::now() > map_deadline {
                        break;
                    }
                    if let Ok(Some(fr)) = task_result {
                        let title = if get_title { fr.title } else { None };
                        let desc = if get_description {
                            fr.description
                        } else {
                            None
                        };
                        meta_map.insert(fr.url, (title, desc));
                    }
                }
                for link in &mut results {
                    if link.title.is_none() && link.description.is_none() {
                        if let Some((title, description)) = meta_map.remove(&link.url) {
                            link.title = title;
                            link.description = description;
                        }
                    }
                }
            }

            frontier = next_frontier;
        }
    }

    let mut links = results;

    // ── Step 4: Apply search filter if provided ──────────────────────────

    if let Some(ref search) = request.search {
        let search_lower = search.to_lowercase();
        links.retain(|link| {
            link.url.to_lowercase().contains(&search_lower)
                || link
                    .title
                    .as_ref()
                    .map(|t| t.to_lowercase().contains(&search_lower))
                    .unwrap_or(false)
                || link
                    .description
                    .as_ref()
                    .map(|d| d.to_lowercase().contains(&search_lower))
                    .unwrap_or(false)
        });

        // Sort: prioritize URLs where the search term appears in the URL path
        links.sort_by(|a, b| {
            let a_match = a.url.to_lowercase().contains(&search_lower);
            let b_match = b.url.to_lowercase().contains(&search_lower);
            b_match.cmp(&a_match)
        });
    }

    links.truncate(limit);
    let total = links.len();

    let duration_ms = start_time.elapsed().as_millis() as u64;
    info!(
        url = %request.url,
        total,
        duration_ms,
        "Map completed"
    );

    // Track map request in ClickHouse request_events
    if let Some(ref batcher) = state.analytics.request_batcher {
        let account_id = account_ctx
            .as_ref()
            .map(|c| c.account_id.clone())
            .unwrap_or_default();
        let domain = extract_domain(&request.url).unwrap_or_default();
        let event = ClickHouseRequestEvent {
            account_id,
            job_id: String::new(),
            operation: "map".to_string(),
            url: request.url.clone(),
            domain,
            status_code: 200,
            duration_ms: duration_ms as u32,
            content_length: 0,
            error: String::new(),
            js_rendered: false,
            ai_summary: false,
            ai_extraction: false,
            ai_prompt_tokens: 0,
            ai_completion_tokens: 0,
            ai_model: String::new(),
            urls_found: total as u32,
            pages_fetched: visited.len() as u32,
            timestamp: time::OffsetDateTime::now_utc(),
        };
        let batcher = batcher.clone();
        tokio::spawn(async move {
            if let Err(e) = batcher.add(event).await {
                debug!(error = %e, "Failed to add map request event to ClickHouse");
            }
        });
    }

    // Deduct 1 credit for successful map (atomic)
    if let (Some(ref pool), Some(ref ctx)) = (&state.db_pool, &account_ctx) {
        if let Err(e) = billing::check_credits_and_deduct(
            pool,
            &ctx.account_id,
            1,
            "map",
            &request.url,
        )
        .await
        {
            warn!(account_id = %ctx.account_id, error = ?e, "Failed to deduct credit for map");
        }
    }

    Ok(Json(MapResponse {
        success: true,
        links,
        total,
        duration_ms,
    }))
}

/// Create a new async crawl job
async fn create_crawl(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Json(config): Json<CrawlConfig>,
) -> Result<Json<CreateCrawlResponse>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;
    Ok(Json(
        do_create_crawl(&state, config, account_ctx.as_ref()).await?,
    ))
}

/// Create a sync crawl job (waits for completion)
async fn create_crawl_sync(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Json(config): Json<CrawlConfig>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;
    // First create the async job
    let response = do_create_crawl(&state, config, account_ctx.as_ref()).await?;
    let job_id = response.job_id.clone();

    // Subscribe to events
    let mut rx = state.crawl.event_tx.subscribe();

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
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;

    // Try in-memory first, fall back to Postgres for historical jobs
    let job = if let Some(job) = state.get_job(&job_id) {
        job
    } else if let Some(ref pool) = state.db_pool {
        if let Some(ctx) = &account_ctx {
            jobs_db::get_job_for_account(pool, &job_id, &ctx.account_id).await
        } else {
            jobs_db::get_job_from_db(pool, &job_id).await
        }
        .ok_or_else(|| ApiError::new("Job not found", "not_found"))?
    } else {
        return Err(ApiError::new("Job not found", "not_found"));
    };

    check_job_ownership(&job, &account_ctx)?;

    Ok(Json(job.into()))
}

/// SSE stream for job events
async fn job_events(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(job_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;

    // Check if job exists and ownership
    let job = state
        .get_job(&job_id)
        .ok_or_else(|| ApiError::new("Job not found", "not_found"))?;
    check_job_ownership(&job, &account_ctx)?;

    let rx = state.crawl.event_tx.subscribe();
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
    let jobs = state.crawl.jobs.read();
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
    let errors_count = state.diagnostics.recent_errors.read().len();
    let counters = state.diagnostics.domain_counters.read();
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
    let meilisearch = std::env::var("MEILISEARCH_URL")
        .ok()
        .map(|url| MeilisearchStats {
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
    let errors = state.diagnostics.recent_errors.read();

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
    let recent: Vec<ErrorRecord> = filtered.into_iter().rev().take(params.last).collect();

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
    let counters = state.diagnostics.domain_counters.read();

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
    Event { job_id: String, event: CrawlEvent },
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
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

/// Handle a WebSocket connection
async fn handle_ws_connection(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Track subscribed job IDs
    let subscriptions: Arc<RwLock<std::collections::HashSet<String>>> =
        Arc::new(RwLock::new(std::collections::HashSet::new()));

    // Subscribe to broadcast channel for events
    let mut event_rx = state.crawl.event_tx.subscribe();

    // Spawn task to forward events to WebSocket
    let subs = subscriptions.clone();
    let send_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok((job_id, event)) => {
                    // Only send if subscribed to this job
                    if subs.read().contains(&job_id) {
                        let msg = WsServerMessage::Event { job_id, event };
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
    let mut event_rx = state.crawl.event_tx.subscribe();
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
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;

    // Check ownership before cancelling
    let existing = state
        .get_job(&job_id)
        .ok_or_else(|| ApiError::new("Job not found", "not_found"))?;
    check_job_ownership(&existing, &account_ctx)?;

    let job = state
        .update_job(&job_id, |j| {
            j.status = JobStatus::Cancelled;
            j.completed_at = Some(chrono::Utc::now());
        })
        .ok_or_else(|| ApiError::new("Job not found", "not_found"))?;

    // Persist cancellation to Postgres
    if let Some(ref pool) = state.db_pool {
        state.crawl.dirty_jobs.write().remove(&job_id);
        let pool = pool.clone();
        let snapshot = job.clone();
        tokio::spawn(async move { jobs_db::update_job_full(&pool, &snapshot).await });
    }

    info!(job_id = %job_id, "Job cancelled");

    Ok(Json(job.into()))
}

/// List all jobs
async fn list_jobs(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Query(params): Query<ListJobsQuery>,
) -> Json<Vec<JobStatusResponse>> {
    let account_ctx =
        extract_account_context(state.db_pool.as_ref(), &account_ext, &user_ext).await;

    // When Postgres is available, query DB for full history (survives restarts)
    // and overlay in-memory data for running jobs (fresher counters).
    let jobs: Vec<JobState> = if let Some(ref pool) = state.db_pool {
        let mut db_jobs = if let Some(ctx) = &account_ctx {
            jobs_db::list_jobs_for_account_db(
                pool,
                &ctx.account_id,
                params.limit as i64,
                params.offset as i64,
            )
            .await
        } else {
            jobs_db::list_all_jobs_db(pool, params.limit as i64, params.offset as i64).await
        };

        // Overlay in-memory state for active jobs (fresher counters)
        let in_memory = state.crawl.jobs.read();
        for job in &mut db_jobs {
            if let Some(mem_job) = in_memory.get(&job.job_id) {
                if matches!(mem_job.status, JobStatus::Running | JobStatus::Pending) {
                    *job = mem_job.clone();
                }
            }
        }
        db_jobs
    } else if let Some(ctx) = &account_ctx {
        // No DB — filter in-memory by account
        let all_jobs = state.crawl.jobs.read();
        all_jobs
            .values()
            .filter(|j| j.account_id.as_deref() == Some(&ctx.account_id))
            .skip(params.offset)
            .take(params.limit)
            .cloned()
            .collect()
    } else {
        state.list_jobs(params.limit, params.offset)
    };
    Json(jobs.into_iter().map(|j| j.into()).collect())
}

// ============================================================================
// Event Consumer
// ============================================================================

/// Start consuming events from a message bus to update job state.
/// Returns a JoinHandle so the caller can await clean shutdown.
fn start_event_consumer(
    consumer: AnyConsumer,
    state: Arc<AppState>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    consumer.subscribe(&[topic_names::EVENTS])?;
    info!("Event consumer subscribed to {} topic", topic_names::EVENTS);

    // Process events in background
    let handle = tokio::spawn(async move {
        loop {
            if *shutdown.borrow() {
                info!("Event consumer shutting down");
                break;
            }
            match consumer
                .poll_one::<CrawlEvent>(Duration::from_millis(100))
                .await
            {
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

/// Initialize ClickHouse storage and event batchers.
/// Returns (AnalyticsState for API, RequestEventBatcher, AiUsageBatcher, JobEventBatcher).
async fn init_clickhouse() -> (
    Option<Arc<analytics::AnalyticsState>>,
    Option<Arc<RequestEventBatcher>>,
    Option<Arc<AiUsageBatcher>>,
    Option<Arc<JobEventBatcher>>,
) {
    // Check if ClickHouse is configured
    let config = match analytics::AnalyticsConfig::from_env() {
        Some(c) => c,
        None => {
            info!("ClickHouse not configured (CLICKHOUSE_URL not set)");
            return (None, None, None, None);
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
            return (None, None, None, None);
        }
    };

    info!(
        url = %config.clickhouse_url,
        database = %config.clickhouse_database,
        "Connected to ClickHouse"
    );

    // Create request event batcher (batch size of 50 — 1 row per API call)
    let batcher = Arc::new(RequestEventBatcher::new(storage.clone(), 50, "request_events"));

    // Create AI usage batcher (batch size of 50 events)
    let ai_batcher = Arc::new(AiUsageBatcher::new(storage.clone(), 50, "ai_usage"));

    // Create job event batcher (batch size of 50 — lifecycle events only)
    let job_batcher = Arc::new(JobEventBatcher::new(storage.clone(), 50, "job_events"));

    // Create analytics state (sharing the same storage connection)
    let analytics_state = Arc::new(analytics::AnalyticsState::with_storage(storage));

    (
        Some(analytics_state),
        Some(batcher),
        Some(ai_batcher),
        Some(job_batcher),
    )
}

// ============================================================================
// Run
// ============================================================================

/// Run the API server with pre-built message bus trait objects.
///
/// This is the primary entry point for both the standalone binary and the `scrapix all`
/// orchestration mode, where the caller injects pre-built `ChannelProducer`/`ChannelConsumer`
/// trait objects instead of Kafka ones.
pub async fn run_with_bus(
    args: Args,
    producer: AnyProducer,
    consumer: AnyConsumer,
) -> anyhow::Result<()> {
    info!(
        host = %args.host,
        port = args.port,
        "Starting Scrapix API server"
    );

    // Initialize ClickHouse for analytics (optional)
    let (analytics_state, request_batcher, ai_usage_batcher, job_event_batcher) =
        init_clickhouse().await;

    // Initialize auth state if DATABASE_URL is provided
    let auth_state = if let Some(ref db_url) = args.database_url {
        match auth::AuthState::new(db_url, args.jwt_secret.clone()).await {
            Ok(state) => {
                // Auto-apply schema (idempotent — safe to run on every startup)
                let schema_sql = include_str!("../../../deploy/postgres/init.sql");
                match sqlx::raw_sql(schema_sql).execute(&state.pool).await {
                    Ok(_) => info!("PostgreSQL schema applied successfully"),
                    Err(e) => warn!(error = %e, "Failed to apply PostgreSQL schema (non-fatal)"),
                }
                info!("Authentication enabled via PostgreSQL");
                Some(Arc::new(state))
            }
            Err(e) => {
                warn!(error = %e, "Failed to connect to PostgreSQL. Auth disabled.");
                None
            }
        }
    } else {
        info!("Authentication disabled (DATABASE_URL not set)");
        None
    };

    // Initialize shared HTTP fetcher for /scrape endpoint
    let robots_config = RobotsConfig {
        respect_robots: false, // /scrape is user-directed, not a crawler
        ..Default::default()
    };
    let robots_cache =
        Arc::new(RobotsCache::new(robots_config).expect("Failed to create robots cache"));
    let fetcher = Arc::new(
        HttpFetcherBuilder::new()
            .with_dns_cache()
            .build(robots_cache)
            .expect("Failed to create HTTP fetcher"),
    );
    info!("Shared HTTP fetcher initialized for /scrape endpoint");

    // Initialize AI service from environment (supports multiple providers via AI_PROVIDER)
    // Use with_tracking when ClickHouse is available for per-call token tracking
    let (ai_service, ai_usage_rx) = if ai_usage_batcher.is_some() {
        match AiClient::from_env_with_tracking() {
            Ok((client, rx)) => {
                let provider =
                    std::env::var("AI_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());
                info!(provider = %provider, "AI service initialized with usage tracking");
                (Some(Arc::new(AiService::new(Arc::new(client)))), Some(rx))
            }
            Err(e) => {
                info!(reason = %e, "AI enrichment disabled");
                (None, None)
            }
        }
    } else {
        match AiClient::from_env() {
            Ok(client) => {
                let provider =
                    std::env::var("AI_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());
                info!(provider = %provider, "AI service initialized");
                (Some(Arc::new(AiService::new(Arc::new(client)))), None)
            }
            Err(e) => {
                info!(reason = %e, "AI enrichment disabled");
                (None, None)
            }
        }
    };

    // Create application state
    let config = AppConfig {
        max_jobs: args.max_jobs,
    };
    let db_pool = auth_state.as_ref().map(|a| a.pool.clone());
    let state = Arc::new(AppState::new(
        producer,
        config,
        request_batcher.clone(),
        ai_usage_batcher.clone(),
        job_event_batcher.clone(),
        fetcher,
        ai_service,
        db_pool,
    ));

    // Recover active jobs from Postgres on startup
    if let Some(ref pool) = state.db_pool {
        let recovered = jobs_db::load_active_jobs(pool).await;
        if !recovered.is_empty() {
            let now = std::time::Instant::now();
            let mut jobs = state.crawl.jobs.write();
            let mut activity = state.crawl.job_last_activity.write();
            for job in &recovered {
                jobs.insert(job.job_id.clone(), job.clone());
                if matches!(job.status, JobStatus::Running) {
                    // Give idle detector a fresh 30s window for recovered running jobs
                    activity.insert(job.job_id.clone(), now);
                }
            }
            info!(
                count = recovered.len(),
                "Recovered active jobs from Postgres"
            );
        }
    }

    // Shutdown coordination
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Start event consumer for real-time job tracking
    let consumer_handle = start_event_consumer(consumer, state.clone(), shutdown_rx.clone())?;
    info!("Event consumer started for centralized job tracking");

    // Spawn AI usage receiver task: drains events from the AiClient channel into the ClickHouse batcher
    let ai_receiver_handle =
        if let (Some(mut rx), Some(ref batcher)) = (ai_usage_rx, &ai_usage_batcher) {
            let batcher = batcher.clone();
            let handle = tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    let ch_event = ClickHouseAiUsageEvent {
                        provider: event.provider,
                        model: event.model,
                        operation: String::new(), // API /scrape doesn't have operation context
                        prompt_tokens: event.prompt_tokens,
                        completion_tokens: event.completion_tokens,
                        total_tokens: event.total_tokens,
                        duration_ms: event.duration_ms as u32,
                        job_id: String::new(),
                        account_id: String::new(),
                        url: String::new(),
                        timestamp: time::OffsetDateTime::now_utc(),
                    };
                    if let Err(e) = batcher.add(ch_event).await {
                        debug!(error = %e, "Failed to add AI usage event to batcher");
                    }
                }
            });
            info!("AI usage tracking receiver started");
            Some(handle)
        } else {
            None
        };

    // Start periodic flush task (ClickHouse batchers + Postgres dirty job counters)
    let has_flush_work = request_batcher.is_some()
        || ai_usage_batcher.is_some()
        || job_event_batcher.is_some()
        || state.db_pool.is_some();
    let flush_handle = if has_flush_work {
        let req_batcher = request_batcher.clone();
        let ai_batcher = ai_usage_batcher.clone();
        let job_batcher = job_event_batcher.clone();
        let flush_state = state.clone();
        let mut shutdown_rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Some(ref b) = req_batcher {
                            if let Err(e) = b.flush().await {
                                warn!(error = %e, "Failed to flush ClickHouse request batcher");
                            }
                        }
                        if let Some(ref b) = ai_batcher {
                            if let Err(e) = b.flush().await {
                                warn!(error = %e, "Failed to flush ClickHouse AI usage batcher");
                            }
                        }
                        if let Some(ref b) = job_batcher {
                            if let Err(e) = b.flush().await {
                                warn!(error = %e, "Failed to flush ClickHouse job event batcher");
                            }
                        }
                        // Flush dirty job counters to Postgres
                        if let Some(ref pool) = flush_state.db_pool {
                            let dirty_ids: Vec<String> =
                                flush_state.crawl.dirty_jobs.write().drain().collect();
                            if !dirty_ids.is_empty() {
                                let snapshots: Vec<JobState> = {
                                    let jobs = flush_state.crawl.jobs.read();
                                    dirty_ids
                                        .iter()
                                        .filter_map(|id| jobs.get(id).cloned())
                                        .collect()
                                };
                                jobs_db::flush_job_counters(pool, &snapshots).await;
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        info!("Flush task shutting down, performing final flush");
                        if let Some(ref b) = req_batcher {
                            if let Err(e) = b.flush().await {
                                warn!(error = %e, "Failed final ClickHouse request flush");
                            }
                        }
                        if let Some(ref b) = ai_batcher {
                            if let Err(e) = b.flush().await {
                                warn!(error = %e, "Failed final ClickHouse AI usage flush");
                            }
                        }
                        if let Some(ref b) = job_batcher {
                            if let Err(e) = b.flush().await {
                                warn!(error = %e, "Failed final ClickHouse job event flush");
                            }
                        }
                        // Final Postgres flush
                        if let Some(ref pool) = flush_state.db_pool {
                            let dirty_ids: Vec<String> =
                                flush_state.crawl.dirty_jobs.write().drain().collect();
                            if !dirty_ids.is_empty() {
                                let snapshots: Vec<JobState> = {
                                    let jobs = flush_state.crawl.jobs.read();
                                    dirty_ids
                                        .iter()
                                        .filter_map(|id| jobs.get(id).cloned())
                                        .collect()
                                };
                                jobs_db::flush_job_counters(pool, &snapshots).await;
                            }
                        }
                        break;
                    }
                }
            }
        });
        if request_batcher.is_some() || ai_usage_batcher.is_some() {
            info!("ClickHouse event persistence enabled (flush interval: 5s)");
        }
        if state.db_pool.is_some() {
            info!("Postgres job counter flush enabled (flush interval: 5s)");
        }
        Some(handle)
    } else {
        None
    };

    // Start idle-job completion detector (checks every 2s, completes jobs idle for 10s)
    let idle_state = state.clone();
    let mut idle_shutdown_rx = shutdown_rx.clone();
    let idle_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = std::time::Instant::now();
                    let idle_threshold = Duration::from_secs(10);

                    // Find running jobs that have been idle
                    // Also extract swap metadata for atomic index swap
                    // (job_id, pages_crawled, pages_indexed, errors, swap_temp_index, swap_target_index, swap_search_api_key, index_uid)
                    type IdleJobInfo = (String, u64, u64, u64, Option<String>, Option<String>, Option<String>, String);
                    let idle_jobs: Vec<IdleJobInfo> = {
                        let jobs = idle_state.crawl.jobs.read();
                        let activity = idle_state.crawl.job_last_activity.read();
                        jobs.iter()
                            .filter(|(_, j)| matches!(j.status, JobStatus::Running))
                            .filter(|(_, j)| j.pages_crawled > 0) // Must have done some work
                            .filter(|(id, _)| {
                                activity.get(*id)
                                    .map(|last| now.duration_since(*last) > idle_threshold)
                                    .unwrap_or(false)
                            })
                            .map(|(id, j)| (
                                id.clone(),
                                j.pages_crawled,
                                j.pages_indexed,
                                j.errors,
                                j.swap_temp_index.clone(),
                                j.swap_meilisearch_url.clone(),
                                j.swap_meilisearch_api_key.clone(),
                                j.index_uid.clone(),
                            ))
                            .collect()
                    };

                    for (job_id, pages_crawled, documents_indexed, errors, swap_temp, swap_url, swap_key, index_uid) in idle_jobs {
                        let duration_secs = {
                            let jobs = idle_state.crawl.jobs.read();
                            jobs.get(&job_id)
                                .and_then(|j| j.duration_seconds())
                                .unwrap_or(0) as u64
                        };

                        // If this job uses atomic index swap, perform the swap before completing
                        if let (Some(temp_index), Some(ms_url)) = (&swap_temp, &swap_url) {
                            info!(
                                job_id = %job_id,
                                target = %index_uid,
                                temp = %temp_index,
                                "Performing atomic index swap before completing job"
                            );

                            match scrapix_storage::meilisearch::MeilisearchStorage::perform_swap(
                                ms_url,
                                swap_key.as_deref(),
                                &index_uid,
                                temp_index,
                            ).await {
                                Ok(()) => {
                                    info!(
                                        job_id = %job_id,
                                        target = %index_uid,
                                        temp = %temp_index,
                                        "Index swap completed successfully"
                                    );
                                }
                                Err(e) => {
                                    error!(
                                        job_id = %job_id,
                                        error = %e,
                                        temp = %temp_index,
                                        "Index swap failed, marking job as failed"
                                    );

                                    let event = CrawlEvent::JobFailed {
                                        job_id: job_id.clone(),
                                        account_id: None,
                                        error: format!(
                                            "Index swap failed: {}. Temp index '{}' preserved for manual recovery.",
                                            e, temp_index
                                        ),
                                        timestamp: chrono::Utc::now().timestamp_millis(),
                                    };
                                    idle_state.process_event(&job_id, &event);
                                    idle_state.broadcast_event(&job_id, event);
                                    idle_state.crawl.job_last_activity.write().remove(&job_id);
                                    continue;
                                }
                            }
                        }

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
                        idle_state.crawl.job_last_activity.write().remove(&job_id);
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

    // Start cron scheduler if database is configured
    let cron_handle = if let Some(ref pool) = state.db_pool {
        let handle =
            configs::spawn_cron_scheduler(state.clone(), pool.clone(), shutdown_rx.clone());
        info!("Cron scheduler started (30s tick interval)");
        Some(handle)
    } else {
        None
    };

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

    // Protected routes (API key auth required when enabled)
    let mut protected_routes = Router::new()
        .route("/scrape", post(scrape_url))
        .route("/map", post(map_url))
        .route("/crawl", post(create_crawl))
        .route("/crawl/sync", post(create_crawl_sync))
        .route("/jobs", get(list_jobs))
        .route("/job/{id}/status", get(job_status))
        .route("/job/{id}/events", get(job_events))
        .route("/job/{id}", delete(cancel_job));

    // Add saved config routes if database is available
    if state.db_pool.is_some() {
        protected_routes = protected_routes
            .route(
                "/configs",
                post(configs::create_config).get(configs::list_configs),
            )
            .route(
                "/configs/{id}",
                get(configs::get_config)
                    .patch(configs::update_config)
                    .delete(configs::delete_config),
            )
            .route("/configs/{id}/trigger", post(configs::trigger_config))
            .route(
                "/engines",
                post(engines::create_engine).get(engines::list_engines),
            )
            .route(
                "/engines/{id}",
                get(engines::get_engine)
                    .patch(engines::update_engine)
                    .delete(engines::delete_engine),
            )
            .route("/engines/{id}/default", post(engines::set_default_engine))
            .route("/engines/{id}/indexes", get(engines::list_engine_indexes));
    }

    // Apply auth middleware if configured (accepts API key or session cookie)
    let protected_routes = if let Some(ref auth) = auth_state {
        protected_routes.layer(middleware::from_fn_with_state(
            auth.clone(),
            auth::validate_api_key_or_session,
        ))
    } else {
        protected_routes
    };

    let mut app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Add auth and session routes if database is configured
    if let Some(ref auth) = auth_state {
        app = app
            .merge(auth::auth_routes(auth.clone()))
            .merge(auth::session_routes(auth.clone()));
        info!("Auth routes enabled (/auth/signup, /auth/login, /auth/me, /account/*)");
    }

    // Add analytics routes if ClickHouse is available
    if let Some(analytics) = analytics_state {
        app = app.nest(
            "/analytics/v0",
            analytics::create_analytics_router(analytics),
        );
        info!("Analytics API enabled at /analytics/v0/pipes");
    } else {
        info!("Analytics API disabled (ClickHouse not available)");
    }

    // CORS: credential-aware
    // When CORS_ORIGINS is set (comma-separated URLs), use those + *.meilisearch.com wildcard.
    // When unset, fall back to localhost defaults.
    let extra_origins: Vec<axum::http::HeaderValue> =
        if let Ok(origins) = std::env::var("CORS_ORIGINS") {
            origins
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        } else {
            Vec::new()
        };
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            let origin_str = origin.to_str().unwrap_or("");
            // Always allow localhost dev
            if origin_str == "http://localhost:3001" || origin_str == "http://127.0.0.1:3001" {
                return true;
            }
            // Allow any *.meilisearch.com subdomain (https only)
            if let Some(host) = origin_str.strip_prefix("https://") {
                if host == "meilisearch.com" || host.ends_with(".meilisearch.com") {
                    return true;
                }
            }
            // Allow explicitly configured origins
            extra_origins.iter().any(|allowed| allowed == origin)
        }))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::COOKIE,
            axum::http::HeaderName::from_static("x-api-key"),
        ])
        .allow_credentials(true)
        .expose_headers([axum::http::header::SET_COOKIE]);
    app = app.layer(cors);

    // Start server with graceful shutdown
    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address '{}:{}': {}", args.host, args.port, e))?;

    // Retry binding in case the previous process hasn't released the port yet (hot-reload)
    let listener = {
        let mut retries = 0u32;
        loop {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => break l,
                Err(e) if retries < 10 => {
                    retries += 1;
                    warn!(
                        "Port {} busy, retrying in 500ms ({}/10): {}",
                        args.port, retries, e
                    );
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
    if let Err(e) = consumer_handle.await {
        warn!("Consumer task failed during shutdown: {}", e);
    }
    if let Err(e) = idle_handle.await {
        warn!("Idle detector task failed during shutdown: {}", e);
    }
    if let Some(handle) = cron_handle {
        if let Err(e) = handle.await {
            warn!("Cron task failed during shutdown: {}", e);
        }
    }
    if let Some(handle) = flush_handle {
        if let Err(e) = handle.await {
            warn!("Flush task failed during shutdown: {}", e);
        }
    }
    if let Some(handle) = ai_receiver_handle {
        handle.abort();
    }
    info!("Shutdown complete");

    Ok(())
}

/// Run the API server using Kafka as the message bus (standard standalone mode).
///
/// Builds Kafka producer and consumer from the broker address in `args`, then delegates
/// to [`run_with_bus`].
pub async fn run(args: Args) -> anyhow::Result<()> {
    // Create Kafka producer
    let producer: AnyProducer = ProducerBuilder::new(&args.brokers)
        .client_id("scrapix-api")
        .compression("lz4")
        .build()?
        .into();

    // Create Kafka consumer for event tracking
    let consumer: AnyConsumer = ConsumerBuilder::new(&args.brokers, "scrapix-api-events")
        .client_id("scrapix-api-event-consumer")
        .auto_offset_reset("latest") // Only process new events
        .build()?
        .into();

    info!(brokers = %args.brokers, "Connected to Kafka");

    run_with_bus(args, producer, consumer).await
}
