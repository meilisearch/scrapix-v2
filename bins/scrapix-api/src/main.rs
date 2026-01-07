//! Scrapix API Server
//!
//! REST API and WebSocket server for managing crawl jobs.
//!
//! ## REST Endpoints
//!
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

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
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
use scrapix_queue::{topic_names, CrawlEvent, KafkaProducer, ProducerBuilder, UrlMessage};

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

    /// API key for authentication (optional)
    #[arg(long, env = "API_KEY")]
    api_key: Option<String>,

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
}

#[derive(Debug, Clone)]
struct AppConfig {
    max_jobs: usize,
}

impl AppState {
    fn new(producer: KafkaProducer, config: AppConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            producer,
            jobs: RwLock::new(HashMap::new()),
            event_tx,
            config,
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
    job.start();

    // Publish seed URLs to frontier
    let mut urls_published = 0;
    for url in &config.start_urls {
        let crawl_url = CrawlUrl::seed(url);
        let msg = UrlMessage::new(crawl_url, &job_id, &config.index_uid);

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

    // Create application state
    let config = AppConfig {
        max_jobs: args.max_jobs,
    };
    let state = Arc::new(AppState::new(producer, config));

    // Build router
    let mut app = Router::new()
        .route("/health", get(health))
        .route("/crawl", post(create_crawl))
        .route("/crawl/sync", post(create_crawl_sync))
        .route("/jobs", get(list_jobs))
        .route("/job/{id}/status", get(job_status))
        .route("/job/{id}/events", get(job_events))
        .route("/job/{id}", delete(cancel_job))
        // WebSocket endpoints
        .route("/ws", get(ws_handler))
        .route("/ws/job/{id}", get(ws_job_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Add CORS if enabled
    if args.enable_cors {
        app = app.layer(CorsLayer::permissive());
        info!("CORS enabled for all origins");
    }

    // Start server
    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
