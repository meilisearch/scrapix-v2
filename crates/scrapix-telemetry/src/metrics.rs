//! # Prometheus Metrics
//!
//! Provides metrics collection and Prometheus exposition for crawler observability.
//!
//! ## Features
//!
//! - Pre-defined crawler metrics (pages fetched, bytes downloaded, errors, etc.)
//! - Prometheus HTTP endpoint for scraping
//! - Custom metric registration
//! - Label support for multi-dimensional metrics
//!
//! ## Example
//!
//! ```rust,ignore
//! use scrapix_telemetry::metrics::{MetricsConfig, MetricsExporter, CrawlerMetrics};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize metrics exporter
//!     let config = MetricsConfig::builder()
//!         .listen_addr("0.0.0.0:9090")
//!         .endpoint("/metrics")
//!         .build();
//!
//!     let exporter = MetricsExporter::new(config)?.install()?;
//!
//!     // Start HTTP server for Prometheus scraping
//!     let handle = exporter.start().await?;
//!
//!     // Record metrics
//!     CrawlerMetrics::record_page_fetched("example.com", 200);
//!     CrawlerMetrics::record_bytes_downloaded(1024);
//!     CrawlerMetrics::record_fetch_duration(Duration::from_millis(150), "example.com");
//!
//!     // Graceful shutdown
//!     handle.shutdown().await;
//!     Ok(())
//! }
//! ```

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use thiserror::Error;
use tokio::sync::oneshot;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during metrics operations.
#[derive(Error, Debug)]
pub enum MetricsError {
    /// Failed to bind to the specified address.
    #[error("Failed to bind metrics server to {addr}: {source}")]
    BindError {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },

    /// Metrics recorder already installed.
    #[error("A metrics recorder has already been installed")]
    RecorderAlreadyInstalled,

    /// Failed to install the metrics recorder.
    #[error("Failed to install metrics recorder: {0}")]
    RecorderInstallError(String),

    /// Server error.
    #[error("Metrics server error: {0}")]
    ServerError(String),
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the metrics exporter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetricsConfig {
    /// Address to listen on for Prometheus scraping.
    pub listen_addr: SocketAddr,

    /// HTTP endpoint path for metrics (default: "/metrics").
    pub endpoint: String,

    /// Whether to include process metrics (memory, CPU, etc.).
    pub include_process_metrics: bool,

    /// Global labels applied to all metrics.
    pub global_labels: Vec<(String, String)>,

    /// Histogram bucket boundaries for duration metrics (in seconds).
    pub duration_buckets: Vec<f64>,

    /// Histogram bucket boundaries for size metrics (in bytes).
    pub size_buckets: Vec<f64>,

    /// Prefix for all metric names.
    pub metric_prefix: Option<String>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:9090".parse().unwrap(),
            endpoint: "/metrics".to_string(),
            include_process_metrics: true,
            global_labels: Vec::new(),
            duration_buckets: default_duration_buckets(),
            size_buckets: default_size_buckets(),
            metric_prefix: Some("scrapix".to_string()),
        }
    }
}

impl MetricsConfig {
    /// Create a new builder for MetricsConfig.
    pub fn builder() -> MetricsConfigBuilder {
        MetricsConfigBuilder::default()
    }
}

/// Default duration histogram buckets (in seconds).
fn default_duration_buckets() -> Vec<f64> {
    vec![
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
    ]
}

/// Default size histogram buckets (in bytes).
fn default_size_buckets() -> Vec<f64> {
    vec![
        1024.0,      // 1 KB
        10240.0,     // 10 KB
        102400.0,    // 100 KB
        524288.0,    // 512 KB
        1048576.0,   // 1 MB
        5242880.0,   // 5 MB
        10485760.0,  // 10 MB
        52428800.0,  // 50 MB
        104857600.0, // 100 MB
    ]
}

/// Builder for MetricsConfig.
#[derive(Debug, Default)]
pub struct MetricsConfigBuilder {
    listen_addr: Option<SocketAddr>,
    endpoint: Option<String>,
    include_process_metrics: Option<bool>,
    global_labels: Vec<(String, String)>,
    duration_buckets: Option<Vec<f64>>,
    size_buckets: Option<Vec<f64>>,
    metric_prefix: Option<String>,
}

impl MetricsConfigBuilder {
    /// Set the listen address.
    pub fn listen_addr(mut self, addr: impl Into<String>) -> Self {
        self.listen_addr = addr.into().parse().ok();
        self
    }

    /// Set the listen address from a SocketAddr.
    pub fn listen_socket_addr(mut self, addr: SocketAddr) -> Self {
        self.listen_addr = Some(addr);
        self
    }

    /// Set the metrics endpoint path.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Enable or disable process metrics.
    pub fn include_process_metrics(mut self, include: bool) -> Self {
        self.include_process_metrics = Some(include);
        self
    }

    /// Add a global label applied to all metrics.
    pub fn global_label(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.global_labels.push((name.into(), value.into()));
        self
    }

    /// Set custom duration histogram buckets (in seconds).
    pub fn duration_buckets(mut self, buckets: Vec<f64>) -> Self {
        self.duration_buckets = Some(buckets);
        self
    }

    /// Set custom size histogram buckets (in bytes).
    pub fn size_buckets(mut self, buckets: Vec<f64>) -> Self {
        self.size_buckets = Some(buckets);
        self
    }

    /// Set the metric name prefix.
    pub fn metric_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.metric_prefix = Some(prefix.into());
        self
    }

    /// Build the MetricsConfig.
    pub fn build(self) -> MetricsConfig {
        let defaults = MetricsConfig::default();
        MetricsConfig {
            listen_addr: self.listen_addr.unwrap_or(defaults.listen_addr),
            endpoint: self.endpoint.unwrap_or(defaults.endpoint),
            include_process_metrics: self
                .include_process_metrics
                .unwrap_or(defaults.include_process_metrics),
            global_labels: if self.global_labels.is_empty() {
                defaults.global_labels
            } else {
                self.global_labels
            },
            duration_buckets: self.duration_buckets.unwrap_or(defaults.duration_buckets),
            size_buckets: self.size_buckets.unwrap_or(defaults.size_buckets),
            metric_prefix: self.metric_prefix.or(defaults.metric_prefix),
        }
    }
}

// ============================================================================
// Metrics Exporter
// ============================================================================

/// Handle to control the running metrics server.
pub struct MetricsServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    is_running: Arc<AtomicBool>,
}

impl MetricsServerHandle {
    /// Check if the server is running.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Shutdown the metrics server.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.is_running.store(false, Ordering::SeqCst);
    }
}

/// Prometheus metrics exporter with HTTP server.
pub struct MetricsExporter {
    config: MetricsConfig,
    handle: Option<PrometheusHandle>,
}

impl MetricsExporter {
    /// Create a new metrics exporter.
    pub fn new(config: MetricsConfig) -> Result<Self, MetricsError> {
        Ok(Self {
            config,
            handle: None,
        })
    }

    /// Install the metrics recorder and return a handle for rendering.
    ///
    /// This should be called once at application startup.
    pub fn install(mut self) -> Result<Self, MetricsError> {
        let mut builder = PrometheusBuilder::new();

        // Set histogram buckets
        builder = builder
            .set_buckets_for_metric(
                Matcher::Suffix("_duration_seconds".to_string()),
                &self.config.duration_buckets,
            )
            .map_err(|e| MetricsError::RecorderInstallError(e.to_string()))?
            .set_buckets_for_metric(
                Matcher::Suffix("_bytes".to_string()),
                &self.config.size_buckets,
            )
            .map_err(|e| MetricsError::RecorderInstallError(e.to_string()))?;

        // Add global labels
        for (name, value) in &self.config.global_labels {
            builder = builder.add_global_label(name.clone(), value.clone());
        }

        // Install the recorder
        let handle = builder
            .install_recorder()
            .map_err(|e| MetricsError::RecorderInstallError(e.to_string()))?;

        self.handle = Some(handle);

        // Register metric descriptions
        register_metric_descriptions();

        Ok(self)
    }

    /// Start the HTTP server for Prometheus scraping.
    ///
    /// Returns a handle that can be used to shut down the server.
    pub async fn start(&self) -> Result<MetricsServerHandle, MetricsError> {
        let handle = self.handle.as_ref().ok_or_else(|| {
            MetricsError::RecorderInstallError(
                "Recorder not installed. Call install() first.".to_string(),
            )
        })?;

        let handle = handle.clone();
        let addr = self.config.listen_addr;
        let endpoint = self.config.endpoint.clone();
        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = is_running.clone();

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            run_metrics_server(addr, endpoint, handle, shutdown_rx, is_running_clone).await;
        });

        Ok(MetricsServerHandle {
            shutdown_tx: Some(shutdown_tx),
            is_running,
        })
    }

    /// Render current metrics as Prometheus text format.
    pub fn render(&self) -> Option<String> {
        self.handle.as_ref().map(|h| h.render())
    }
}

/// Simple HTTP server for metrics endpoint.
async fn run_metrics_server(
    addr: SocketAddr,
    endpoint: String,
    handle: PrometheusHandle,
    mut shutdown_rx: oneshot::Receiver<()>,
    is_running: Arc<AtomicBool>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind metrics server to {}: {}", addr, e);
            is_running.store(false, Ordering::SeqCst);
            return;
        }
    };

    tracing::info!("Metrics server listening on http://{}{}", addr, endpoint);

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!("Metrics server shutting down");
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((mut socket, _)) => {
                        let handle = handle.clone();
                        let endpoint = endpoint.clone();
                        tokio::spawn(async move {
                            let mut buf = [0u8; 1024];
                            if let Ok(n) = socket.read(&mut buf).await {
                                let request = String::from_utf8_lossy(&buf[..n]);

                                // Simple HTTP request parsing
                                let is_metrics_request = request
                                    .lines()
                                    .next()
                                    .map(|line| line.contains(&endpoint) || line.contains("/metrics"))
                                    .unwrap_or(false);

                                let response = if is_metrics_request {
                                    let body = handle.render();
                                    format!(
                                        "HTTP/1.1 200 OK\r\n\
                                         Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n\
                                         Content-Length: {}\r\n\
                                         \r\n\
                                         {}",
                                        body.len(),
                                        body
                                    )
                                } else {
                                    "HTTP/1.1 404 Not Found\r\n\
                                     Content-Length: 0\r\n\
                                     \r\n"
                                        .to_string()
                                };

                                let _ = socket.write_all(response.as_bytes()).await;
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to accept connection: {}", e);
                    }
                }
            }
        }
    }

    is_running.store(false, Ordering::SeqCst);
}

// ============================================================================
// Metric Registration
// ============================================================================

/// Register all metric descriptions.
///
/// This provides documentation for metrics when viewing /metrics endpoint.
fn register_metric_descriptions() {
    // Fetch metrics
    describe_counter!(
        "scrapix_pages_fetched_total",
        "Total number of pages fetched"
    );
    describe_counter!("scrapix_bytes_downloaded_total", "Total bytes downloaded");
    describe_histogram!(
        "scrapix_fetch_duration_seconds",
        "Time taken to fetch a page"
    );
    describe_counter!("scrapix_fetch_errors_total", "Total fetch errors by type");

    // Processing metrics
    describe_counter!(
        "scrapix_pages_processed_total",
        "Total pages processed successfully"
    );
    describe_counter!(
        "scrapix_documents_extracted_total",
        "Total documents extracted"
    );
    describe_histogram!(
        "scrapix_processing_duration_seconds",
        "Time taken to process a page"
    );

    // Queue metrics
    describe_gauge!("scrapix_queue_size", "Current number of URLs in the queue");
    describe_gauge!(
        "scrapix_active_fetches",
        "Number of currently active fetch operations"
    );
    describe_counter!(
        "scrapix_urls_discovered_total",
        "Total URLs discovered during crawling"
    );
    describe_counter!(
        "scrapix_urls_filtered_total",
        "Total URLs filtered out by pattern matching"
    );

    // Robots.txt metrics
    describe_counter!(
        "scrapix_robots_checks_total",
        "Total robots.txt checks performed"
    );
    describe_counter!(
        "scrapix_robots_blocks_total",
        "Total URLs blocked by robots.txt"
    );
    describe_histogram!(
        "scrapix_robots_fetch_duration_seconds",
        "Time to fetch robots.txt"
    );

    // DNS metrics
    describe_counter!("scrapix_dns_lookups_total", "Total DNS lookups performed");
    describe_counter!("scrapix_dns_cache_hits_total", "DNS cache hits");
    describe_histogram!(
        "scrapix_dns_lookup_duration_seconds",
        "Time taken for DNS lookups"
    );

    // Connection metrics
    describe_gauge!(
        "scrapix_connections_active",
        "Current active HTTP connections"
    );
    describe_counter!(
        "scrapix_connections_total",
        "Total HTTP connections established"
    );

    // Extraction metrics
    describe_counter!(
        "scrapix_links_extracted_total",
        "Total links extracted from pages"
    );
    describe_histogram!(
        "scrapix_extraction_duration_seconds",
        "Time taken for content extraction"
    );

    // AI/LLM metrics
    describe_counter!("scrapix_ai_requests_total", "Total AI/LLM requests made");
    describe_counter!(
        "scrapix_ai_tokens_used_total",
        "Total tokens used for AI processing"
    );
    describe_histogram!(
        "scrapix_ai_request_duration_seconds",
        "Time taken for AI requests"
    );

    // Storage metrics
    describe_counter!(
        "scrapix_storage_writes_total",
        "Total storage write operations"
    );
    describe_counter!(
        "scrapix_storage_bytes_written_total",
        "Total bytes written to storage"
    );
    describe_histogram!(
        "scrapix_storage_write_duration_seconds",
        "Time taken for storage writes"
    );

    // Browser rendering metrics
    describe_counter!(
        "scrapix_browser_renders_total",
        "Total browser render operations"
    );
    describe_histogram!(
        "scrapix_browser_render_duration_seconds",
        "Time taken for browser rendering"
    );
    describe_gauge!(
        "scrapix_browser_instances_active",
        "Number of active browser instances"
    );
}

// ============================================================================
// Crawler Metrics Helper
// ============================================================================

/// Pre-defined crawler metrics with convenient recording methods.
///
/// This provides a type-safe interface for recording common crawler metrics.
pub struct CrawlerMetrics;

impl CrawlerMetrics {
    // -------------------------------------------------------------------------
    // Fetch Metrics
    // -------------------------------------------------------------------------

    /// Record a page fetch.
    pub fn record_page_fetched(domain: &str, status_code: u16) {
        counter!(
            "scrapix_pages_fetched_total",
            "domain" => domain.to_string(),
            "status" => status_code.to_string()
        )
        .increment(1);
    }

    /// Record bytes downloaded.
    pub fn record_bytes_downloaded(bytes: u64) {
        counter!("scrapix_bytes_downloaded_total").increment(bytes);
    }

    /// Record fetch duration.
    pub fn record_fetch_duration(duration: Duration, domain: &str) {
        histogram!(
            "scrapix_fetch_duration_seconds",
            "domain" => domain.to_string()
        )
        .record(duration.as_secs_f64());
    }

    /// Record a fetch error.
    pub fn record_fetch_error(error_type: &str, domain: &str) {
        counter!(
            "scrapix_fetch_errors_total",
            "error_type" => error_type.to_string(),
            "domain" => domain.to_string()
        )
        .increment(1);
    }

    // -------------------------------------------------------------------------
    // Processing Metrics
    // -------------------------------------------------------------------------

    /// Record a processed page.
    pub fn record_page_processed(domain: &str) {
        counter!(
            "scrapix_pages_processed_total",
            "domain" => domain.to_string()
        )
        .increment(1);
    }

    /// Record extracted documents.
    pub fn record_documents_extracted(count: u64, doc_type: &str) {
        counter!(
            "scrapix_documents_extracted_total",
            "type" => doc_type.to_string()
        )
        .increment(count);
    }

    /// Record processing duration.
    pub fn record_processing_duration(duration: Duration, stage: &str) {
        histogram!(
            "scrapix_processing_duration_seconds",
            "stage" => stage.to_string()
        )
        .record(duration.as_secs_f64());
    }

    // -------------------------------------------------------------------------
    // Queue Metrics
    // -------------------------------------------------------------------------

    /// Set current queue size.
    pub fn set_queue_size(size: f64, priority: &str) {
        gauge!(
            "scrapix_queue_size",
            "priority" => priority.to_string()
        )
        .set(size);
    }

    /// Set active fetches count.
    pub fn set_active_fetches(count: f64) {
        gauge!("scrapix_active_fetches").set(count);
    }

    /// Record discovered URLs.
    pub fn record_urls_discovered(count: u64, source: &str) {
        counter!(
            "scrapix_urls_discovered_total",
            "source" => source.to_string()
        )
        .increment(count);
    }

    /// Record filtered URLs.
    pub fn record_urls_filtered(count: u64, reason: &str) {
        counter!(
            "scrapix_urls_filtered_total",
            "reason" => reason.to_string()
        )
        .increment(count);
    }

    // -------------------------------------------------------------------------
    // Robots.txt Metrics
    // -------------------------------------------------------------------------

    /// Record a robots.txt check.
    pub fn record_robots_check(allowed: bool, domain: &str) {
        counter!(
            "scrapix_robots_checks_total",
            "allowed" => allowed.to_string(),
            "domain" => domain.to_string()
        )
        .increment(1);

        if !allowed {
            counter!(
                "scrapix_robots_blocks_total",
                "domain" => domain.to_string()
            )
            .increment(1);
        }
    }

    /// Record robots.txt fetch duration.
    pub fn record_robots_fetch_duration(duration: Duration, domain: &str) {
        histogram!(
            "scrapix_robots_fetch_duration_seconds",
            "domain" => domain.to_string()
        )
        .record(duration.as_secs_f64());
    }

    // -------------------------------------------------------------------------
    // DNS Metrics
    // -------------------------------------------------------------------------

    /// Record a DNS lookup.
    pub fn record_dns_lookup(cache_hit: bool) {
        counter!("scrapix_dns_lookups_total").increment(1);
        if cache_hit {
            counter!("scrapix_dns_cache_hits_total").increment(1);
        }
    }

    /// Record DNS lookup duration.
    pub fn record_dns_lookup_duration(duration: Duration) {
        histogram!("scrapix_dns_lookup_duration_seconds").record(duration.as_secs_f64());
    }

    // -------------------------------------------------------------------------
    // Connection Metrics
    // -------------------------------------------------------------------------

    /// Set active connections count.
    pub fn set_active_connections(count: f64) {
        gauge!("scrapix_connections_active").set(count);
    }

    /// Record a new connection.
    pub fn record_connection_established() {
        counter!("scrapix_connections_total").increment(1);
    }

    // -------------------------------------------------------------------------
    // Extraction Metrics
    // -------------------------------------------------------------------------

    /// Record extracted links.
    pub fn record_links_extracted(count: u64, link_type: &str) {
        counter!(
            "scrapix_links_extracted_total",
            "type" => link_type.to_string()
        )
        .increment(count);
    }

    /// Record extraction duration.
    pub fn record_extraction_duration(duration: Duration, extractor: &str) {
        histogram!(
            "scrapix_extraction_duration_seconds",
            "extractor" => extractor.to_string()
        )
        .record(duration.as_secs_f64());
    }

    // -------------------------------------------------------------------------
    // AI/LLM Metrics
    // -------------------------------------------------------------------------

    /// Record an AI request.
    pub fn record_ai_request(model: &str, success: bool) {
        counter!(
            "scrapix_ai_requests_total",
            "model" => model.to_string(),
            "success" => success.to_string()
        )
        .increment(1);
    }

    /// Record AI tokens used.
    pub fn record_ai_tokens(input_tokens: u64, output_tokens: u64, model: &str) {
        counter!(
            "scrapix_ai_tokens_used_total",
            "type" => "input".to_string(),
            "model" => model.to_string()
        )
        .increment(input_tokens);
        counter!(
            "scrapix_ai_tokens_used_total",
            "type" => "output".to_string(),
            "model" => model.to_string()
        )
        .increment(output_tokens);
    }

    /// Record AI request duration.
    pub fn record_ai_request_duration(duration: Duration, model: &str) {
        histogram!(
            "scrapix_ai_request_duration_seconds",
            "model" => model.to_string()
        )
        .record(duration.as_secs_f64());
    }

    // -------------------------------------------------------------------------
    // Storage Metrics
    // -------------------------------------------------------------------------

    /// Record a storage write.
    pub fn record_storage_write(bytes: u64, backend: &str) {
        counter!(
            "scrapix_storage_writes_total",
            "backend" => backend.to_string()
        )
        .increment(1);
        counter!(
            "scrapix_storage_bytes_written_total",
            "backend" => backend.to_string()
        )
        .increment(bytes);
    }

    /// Record storage write duration.
    pub fn record_storage_write_duration(duration: Duration, backend: &str) {
        histogram!(
            "scrapix_storage_write_duration_seconds",
            "backend" => backend.to_string()
        )
        .record(duration.as_secs_f64());
    }

    // -------------------------------------------------------------------------
    // Browser Rendering Metrics
    // -------------------------------------------------------------------------

    /// Record a browser render operation.
    pub fn record_browser_render(success: bool, renderer: &str) {
        counter!(
            "scrapix_browser_renders_total",
            "success" => success.to_string(),
            "renderer" => renderer.to_string()
        )
        .increment(1);
    }

    /// Record browser render duration.
    pub fn record_browser_render_duration(duration: Duration, renderer: &str) {
        histogram!(
            "scrapix_browser_render_duration_seconds",
            "renderer" => renderer.to_string()
        )
        .record(duration.as_secs_f64());
    }

    /// Set active browser instances count.
    pub fn set_browser_instances(count: f64, renderer: &str) {
        gauge!(
            "scrapix_browser_instances_active",
            "renderer" => renderer.to_string()
        )
        .set(count);
    }
}

// ============================================================================
// Timing Guard
// ============================================================================

/// A guard that records duration when dropped.
///
/// Useful for timing operations with automatic recording on scope exit.
///
/// ## Example
///
/// ```rust,ignore
/// let _timer = TimingGuard::new(|d| {
///     CrawlerMetrics::record_fetch_duration(d, "example.com");
/// });
/// // ... do work ...
/// // Duration is recorded when `_timer` goes out of scope
/// ```
pub struct TimingGuard<F>
where
    F: FnOnce(Duration),
{
    start: std::time::Instant,
    recorder: Option<F>,
}

impl<F> TimingGuard<F>
where
    F: FnOnce(Duration),
{
    /// Create a new timing guard with the given recorder function.
    pub fn new(recorder: F) -> Self {
        Self {
            start: std::time::Instant::now(),
            recorder: Some(recorder),
        }
    }

    /// Finish timing and record the duration manually.
    pub fn finish(mut self) -> Duration {
        let duration = self.start.elapsed();
        if let Some(recorder) = self.recorder.take() {
            recorder(duration);
        }
        duration
    }
}

impl<F> Drop for TimingGuard<F>
where
    F: FnOnce(Duration),
{
    fn drop(&mut self) {
        if let Some(recorder) = self.recorder.take() {
            recorder(self.start.elapsed());
        }
    }
}

// ============================================================================
// Convenience Macros
// ============================================================================

/// Time an async operation and record its duration.
///
/// ## Example
///
/// ```rust,ignore
/// let result = time_operation!(
///     "fetch",
///     |d| CrawlerMetrics::record_fetch_duration(d, "example.com"),
///     async {
///         client.get(url).await
///     }
/// );
/// ```
#[macro_export]
macro_rules! time_operation {
    ($name:expr, $recorder:expr, $op:expr) => {{
        let start = std::time::Instant::now();
        let result = $op;
        let duration = start.elapsed();
        $recorder(duration);
        tracing::debug!(
            operation = $name,
            duration_ms = duration.as_millis(),
            "Operation completed"
        );
        result
    }};
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = MetricsConfig::builder()
            .listen_addr("127.0.0.1:9091")
            .endpoint("/custom-metrics")
            .include_process_metrics(false)
            .global_label("env", "test")
            .metric_prefix("test_scrapix")
            .build();

        assert_eq!(config.listen_addr.to_string(), "127.0.0.1:9091");
        assert_eq!(config.endpoint, "/custom-metrics");
        assert!(!config.include_process_metrics);
        assert_eq!(config.global_labels.len(), 1);
        assert_eq!(
            config.global_labels[0],
            ("env".to_string(), "test".to_string())
        );
        assert_eq!(config.metric_prefix, Some("test_scrapix".to_string()));
    }

    #[test]
    fn test_default_config() {
        let config = MetricsConfig::default();

        assert_eq!(config.listen_addr.to_string(), "0.0.0.0:9090");
        assert_eq!(config.endpoint, "/metrics");
        assert!(config.include_process_metrics);
        assert!(config.global_labels.is_empty());
        assert_eq!(config.metric_prefix, Some("scrapix".to_string()));
    }

    #[test]
    fn test_default_buckets() {
        let duration_buckets = default_duration_buckets();
        let size_buckets = default_size_buckets();

        // Duration buckets should range from 5ms to 60s
        assert_eq!(duration_buckets.first(), Some(&0.005));
        assert_eq!(duration_buckets.last(), Some(&60.0));

        // Size buckets should range from 1KB to 100MB
        assert_eq!(size_buckets.first(), Some(&1024.0));
        assert_eq!(size_buckets.last(), Some(&104857600.0));
    }

    #[test]
    fn test_timing_guard_finish() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let recorded_ms = Arc::new(AtomicU64::new(0));
        let recorded_ms_clone = recorded_ms.clone();

        let timer = TimingGuard::new(move |d: Duration| {
            recorded_ms_clone.store(d.as_millis() as u64, Ordering::SeqCst);
        });

        std::thread::sleep(Duration::from_millis(10));
        let duration = timer.finish();

        assert!(duration.as_millis() >= 10);
        assert!(recorded_ms.load(Ordering::SeqCst) >= 10);
    }

    #[test]
    fn test_timing_guard_drop() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let was_called = Arc::new(AtomicBool::new(false));
        let was_called_clone = was_called.clone();

        {
            let _timer = TimingGuard::new(move |_d: Duration| {
                was_called_clone.store(true, Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(was_called.load(Ordering::SeqCst));
    }
}
