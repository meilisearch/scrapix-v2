//! Scrapix CLI
//!
//! Command-line interface for managing crawl jobs.
//!
//! ## Usage
//!
//! ```bash
//! # Start a crawl job from config file
//! scrapix crawl -p config.json
//!
//! # Start a crawl job with inline config
//! scrapix crawl -c '{"start_urls":["https://example.com"],"index_uid":"my_index"}'
//!
//! # Check job status
//! scrapix status <job-id>
//!
//! # Stream job events
//! scrapix events <job-id>
//!
//! # List recent jobs
//! scrapix jobs
//!
//! # Cancel a job
//! scrapix cancel <job-id>
//!
//! # Validate a configuration file
//! scrapix validate config.json
//!
//! # Run a local crawl (without Kafka)
//! scrapix local -p config.json --output results.json
//! ```

use std::collections::{HashSet, VecDeque};
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tabled::{Table, Tabled};
use tokio::sync::Mutex;
use tracing::debug;

use scrapix_core::CrawlConfig;
use scrapix_extractor::Extractor;
use scrapix_parser::{extract_content, html_to_markdown};

/// Scrapix web crawler CLI
#[derive(Parser, Debug)]
#[command(name = "scrapix")]
#[command(about = "Scrapix web crawler CLI - manage crawl jobs")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    /// API server URL
    #[arg(
        short,
        long,
        env = "SCRAPIX_API_URL",
        default_value = "http://localhost:8080"
    )]
    pub api_url: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    pub output: OutputFormat,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start a new crawl job
    Crawl {
        /// Configuration file path (JSON)
        #[arg(short = 'p', long, group = "config_source")]
        config_path: Option<String>,

        /// Inline JSON configuration
        #[arg(short, long, group = "config_source")]
        config: Option<String>,

        /// Run synchronously (wait for completion)
        #[arg(long)]
        sync: bool,

        /// Follow events after job starts (async mode only)
        #[arg(short, long)]
        follow: bool,
    },

    /// Check job status
    Status {
        /// Job ID
        job_id: String,

        /// Watch mode - continuously poll status
        #[arg(short, long)]
        watch: bool,

        /// Poll interval in seconds (for watch mode)
        #[arg(long, default_value = "2")]
        interval: u64,
    },

    /// Stream job events (SSE)
    Events {
        /// Job ID
        job_id: String,
    },

    /// List recent jobs
    Jobs {
        /// Maximum number of jobs to list
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: usize,
    },

    /// Cancel a running job
    Cancel {
        /// Job ID
        job_id: String,
    },

    /// Check API server health
    Health,

    /// Validate a configuration file
    Validate {
        /// Configuration file path (JSON)
        config_path: String,

        /// Output validation details
        #[arg(short, long)]
        verbose: bool,
    },

    /// Run a local crawl (without Kafka infrastructure)
    Local {
        /// Configuration file path (JSON)
        #[arg(short = 'p', long, group = "config_source")]
        config_path: Option<String>,

        /// Inline JSON configuration
        #[arg(short, long, group = "config_source")]
        config: Option<String>,

        /// Output file for results (JSON)
        #[arg(short, long)]
        output: Option<String>,

        /// Maximum concurrent requests
        #[arg(long, default_value = "10")]
        concurrency: usize,

        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show system-wide statistics
    Stats {
        /// Include verbose details
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show recent errors
    Errors {
        /// Number of recent errors to show
        #[arg(long, default_value = "20")]
        last: usize,

        /// Filter by job ID
        #[arg(long)]
        job: Option<String>,
    },

    /// Show per-domain statistics
    Domains {
        /// Number of top domains to show
        #[arg(long, default_value = "20")]
        top: usize,

        /// Filter by domain pattern
        #[arg(long)]
        filter: Option<String>,
    },

    /// Analytics commands (requires ClickHouse)
    #[command(subcommand)]
    Analytics(AnalyticsCommands),

    /// Run benchmarks
    #[command(subcommand)]
    Bench(BenchCommands),

    /// Kubernetes deployment management
    #[command(subcommand)]
    K8s(K8sCommands),

    /// Local infrastructure management (Docker Compose)
    #[command(subcommand)]
    Infra(InfraCommands),
}

#[derive(Subcommand, Debug)]
pub enum AnalyticsCommands {
    /// List available analytics pipes
    Pipes,

    /// Key performance indicators summary
    Kpis {
        /// Time window in hours
        #[arg(long, default_value = "24")]
        hours: u32,
    },

    /// Top domains by request count
    TopDomains {
        /// Time window in hours
        #[arg(long, default_value = "24")]
        hours: u32,

        /// Number of domains to show
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Statistics for a specific domain
    DomainStats {
        /// Domain to query
        #[arg(long)]
        domain: String,

        /// Time window in hours
        #[arg(long, default_value = "24")]
        hours: u32,
    },

    /// Hourly crawl statistics
    Hourly {
        /// Time window in hours
        #[arg(long, default_value = "24")]
        hours: u32,
    },

    /// Error distribution by status code
    ErrorDist {
        /// Time window in hours
        #[arg(long, default_value = "24")]
        hours: u32,
    },

    /// Statistics for a specific job
    JobStats {
        /// Job ID to query
        #[arg(long)]
        job_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum BenchCommands {
    /// Run all benchmarks
    All {
        /// Output directory for results
        #[arg(short, long, default_value = "./bench-results")]
        output: String,

        /// Number of iterations
        #[arg(short, long, default_value = "1")]
        iterations: u32,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Run Wikipedia end-to-end benchmark
    Wikipedia {
        /// Output directory for results
        #[arg(short, long, default_value = "./bench-results")]
        output: String,

        /// Number of iterations
        #[arg(short, long, default_value = "1")]
        iterations: u32,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Run integrated component benchmarks
    Integrated {
        /// Output directory for results
        #[arg(short, long, default_value = "./bench-results")]
        output: String,

        /// Number of iterations
        #[arg(short, long, default_value = "1")]
        iterations: u32,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Run parser benchmarks
    Parser {
        /// Output directory for results
        #[arg(short, long, default_value = "./bench-results")]
        output: String,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum K8sCommands {
    /// Deploy all services to Kubernetes
    Deploy {
        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,

        /// Kustomize overlay (local, staging, prod, scaleway)
        #[arg(short, long, default_value = "local")]
        overlay: String,
    },

    /// Remove all services from Kubernetes
    Destroy {
        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,

        /// Kustomize overlay (local, staging, prod, scaleway)
        #[arg(short, long, default_value = "local")]
        overlay: String,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show deployment status
    Status {
        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,

        /// Watch status continuously
        #[arg(short, long)]
        watch: bool,
    },

    /// Show logs for a component
    Logs {
        /// Component to show logs for (api, frontier, crawler, content, all)
        #[arg(default_value = "all")]
        component: String,

        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,

        /// Follow logs
        #[arg(short, long)]
        follow: bool,
    },

    /// Scale a component
    Scale {
        /// Component to scale (api, frontier, crawler, content)
        component: String,

        /// Number of replicas
        #[arg(short, long)]
        replicas: u32,

        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
    },

    /// Restart a component
    Restart {
        /// Component to restart (api, frontier, crawler, content, all)
        #[arg(default_value = "all")]
        component: String,

        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
    },

    /// Forward ports for local access
    PortForward {
        /// Kubernetes namespace
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum InfraCommands {
    /// Start infrastructure services (Redpanda, Meilisearch, DragonflyDB)
    Up,

    /// Stop infrastructure services
    Down,

    /// Restart infrastructure services
    Restart,

    /// Show status of infrastructure services
    Status,

    /// Show logs for infrastructure services
    Logs {
        /// Service to show logs for (optional)
        service: Option<String>,

        /// Follow logs
        #[arg(short, long)]
        follow: bool,
    },

    /// Stop and remove all data volumes
    Reset {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

// ============================================================================
// API Response Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
struct CreateCrawlResponse {
    job_id: String,
    status: String,
    index_uid: String,
    start_urls_count: usize,
    #[allow(dead_code)]
    message: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct JobStatusResponse {
    job_id: String,
    status: String,
    index_uid: String,
    pages_crawled: u64,
    pages_indexed: u64,
    documents_sent: u64,
    errors: u64,
    started_at: Option<String>,
    completed_at: Option<String>,
    duration_seconds: Option<i64>,
    error_message: Option<String>,
    crawl_rate: f64,
    eta_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    kafka_connected: bool,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: String,
    code: String,
    #[serde(default)]
    #[allow(dead_code)]
    details: Option<serde_json::Value>,
}

// For table display
#[derive(Tabled)]
struct JobRow {
    #[tabled(rename = "Job ID")]
    job_id: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Index")]
    index_uid: String,
    #[tabled(rename = "Crawled")]
    pages_crawled: u64,
    #[tabled(rename = "Indexed")]
    pages_indexed: u64,
    #[tabled(rename = "Errors")]
    errors: u64,
}

impl From<JobStatusResponse> for JobRow {
    fn from(job: JobStatusResponse) -> Self {
        Self {
            job_id: if job.job_id.len() > 8 {
                format!("{}...", &job.job_id[..8])
            } else {
                job.job_id
            },
            status: job.status,
            index_uid: job.index_uid,
            pages_crawled: job.pages_crawled,
            pages_indexed: job.pages_indexed,
            errors: job.errors,
        }
    }
}

// ============================================================================
// Diagnostic Response Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
struct SystemStatsResponse {
    meilisearch: Option<MeilisearchStats>,
    jobs: JobSummary,
    diagnostics: DiagnosticsStats,
    collected_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct MeilisearchStats {
    available: bool,
    url: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct JobSummary {
    total: usize,
    running: usize,
    completed: usize,
    failed: usize,
    pending: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiagnosticsStats {
    recent_errors_count: usize,
    tracked_domains: usize,
    total_requests: u64,
    total_successes: u64,
    total_failures: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ErrorsResponse {
    errors: Vec<ErrorRecord>,
    total_count: usize,
    by_status: std::collections::HashMap<String, u64>,
    by_domain: Vec<(String, u64)>,
    source: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ErrorRecord {
    url: String,
    domain: String,
    error: String,
    status_code: Option<u16>,
    job_id: String,
    timestamp: String,
    retry_count: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct DomainsResponse {
    domains: Vec<DomainInfo>,
    total_domains: usize,
    source: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct DomainInfo {
    domain: String,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    avg_response_time_ms: Option<f64>,
}

// For table display
#[derive(Tabled)]
struct DomainRow {
    #[tabled(rename = "Domain")]
    domain: String,
    #[tabled(rename = "Requests")]
    requests: u64,
    #[tabled(rename = "Success")]
    success: String,
    #[tabled(rename = "Failed")]
    failed: u64,
    #[tabled(rename = "Avg Time")]
    avg_time: String,
}

// ============================================================================
// Analytics Response Types (Tinybird-style)
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
struct AnalyticsResponse<T> {
    meta: Vec<ColumnMeta>,
    data: Vec<T>,
    rows: usize,
    statistics: QueryStats,
}

#[derive(Debug, Deserialize, Serialize)]
struct ColumnMeta {
    name: String,
    #[serde(rename = "type")]
    col_type: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct QueryStats {
    elapsed: f64,
    rows_read: usize,
    bytes_read: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct PipeInfo {
    name: String,
    description: String,
    endpoint: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct KpisData {
    total_crawls: u64,
    total_bytes: u64,
    unique_domains: u64,
    success_rate: f64,
    avg_response_time_ms: f64,
    errors_count: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct TopDomainData {
    domain: String,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    success_rate: f64,
    avg_response_time_ms: f64,
    total_bytes: u64,
    unique_urls: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct HourlyStatsData {
    hour: String,
    requests: u64,
    successes: u64,
    failures: u64,
    success_rate: f64,
    avg_response_time_ms: f64,
    total_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct ErrorDistData {
    status_code: u16,
    count: u64,
    percentage: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct JobStatsData {
    job_id: String,
    total_urls: u64,
    successful_urls: u64,
    failed_urls: u64,
    success_rate: f64,
    total_bytes: u64,
    avg_response_time_ms: f64,
    unique_domains: u64,
    started_at: String,
    last_activity_at: String,
    duration_seconds: i64,
}

// Table display for analytics
#[derive(Tabled)]
struct TopDomainAnalyticsRow {
    #[tabled(rename = "Domain")]
    domain: String,
    #[tabled(rename = "Requests")]
    requests: u64,
    #[tabled(rename = "Success")]
    success: String,
    #[tabled(rename = "Failed")]
    failed: u64,
    #[tabled(rename = "Avg Time")]
    avg_time: String,
    #[tabled(rename = "Bytes")]
    bytes: String,
}

#[derive(Tabled)]
struct HourlyRow {
    #[tabled(rename = "Hour")]
    hour: String,
    #[tabled(rename = "Requests")]
    requests: u64,
    #[tabled(rename = "Success")]
    success: String,
    #[tabled(rename = "Failed")]
    failed: u64,
    #[tabled(rename = "Avg Time")]
    avg_time: String,
}

#[derive(Tabled)]
struct ErrorDistRow {
    #[tabled(rename = "Status")]
    status: u16,
    #[tabled(rename = "Count")]
    count: u64,
    #[tabled(rename = "Percentage")]
    percentage: String,
}

// ============================================================================
// API Client
// ============================================================================

struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    async fn create_crawl(&self, config: &CrawlConfig, sync: bool) -> Result<CreateCrawlResponse> {
        let endpoint = if sync { "/crawl/sync" } else { "/crawl" };
        let url = format!("{}{}", self.base_url, endpoint);

        let response = self
            .client
            .post(&url)
            .json(config)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    async fn get_status(&self, job_id: &str) -> Result<JobStatusResponse> {
        let url = format!("{}/job/{}/status", self.base_url, job_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    async fn list_jobs(&self, limit: usize, offset: usize) -> Result<Vec<JobStatusResponse>> {
        let url = format!("{}/jobs?limit={}&offset={}", self.base_url, limit, offset);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    async fn cancel_job(&self, job_id: &str) -> Result<JobStatusResponse> {
        let url = format!("{}/job/{}", self.base_url, job_id);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    async fn health(&self) -> Result<HealthResponse> {
        let url = format!("{}/health", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Health check failed")
        }
    }

    async fn stream_events(
        &self,
        job_id: &str,
    ) -> Result<impl futures::Stream<Item = Result<String>>> {
        let url = format!("{}/job/{}/events", self.base_url, job_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if !response.status().is_success() {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }

        // Return a stream that reads SSE events
        Ok(futures::stream::unfold(
            response,
            |mut response| async move {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        let text = String::from_utf8_lossy(&chunk).to_string();
                        Some((Ok(text), response))
                    }
                    Ok(None) => None,
                    Err(e) => Some((Err(anyhow::anyhow!("Stream error: {}", e)), response)),
                }
            },
        ))
    }

    async fn get_stats(&self) -> Result<SystemStatsResponse> {
        let url = format!("{}/stats", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    async fn get_errors(&self, last: usize, job_id: Option<&str>) -> Result<ErrorsResponse> {
        let mut url = format!("{}/errors?last={}", self.base_url, last);
        if let Some(job) = job_id {
            url.push_str(&format!("&job_id={}", job));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    async fn get_domains(&self, top: usize, filter: Option<&str>) -> Result<DomainsResponse> {
        let mut url = format!("{}/domains?top={}", self.base_url, top);
        if let Some(f) = filter {
            url.push_str(&format!("&filter={}", urlencoding::encode(f)));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            let error: ApiError = response.json().await.context("Failed to parse error")?;
            anyhow::bail!("{}: {}", error.code, error.error)
        }
    }

    // Analytics API methods

    async fn analytics_pipes(&self) -> Result<Vec<PipeInfo>> {
        let url = format!("{}/analytics/v0/pipes", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server (is ClickHouse enabled?)")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics API not available (ClickHouse may not be configured)")
        }
    }

    async fn analytics_kpis(&self, hours: u32) -> Result<AnalyticsResponse<KpisData>> {
        let url = format!(
            "{}/analytics/v0/pipes/kpis.json?hours={}",
            self.base_url, hours
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics query failed")
        }
    }

    async fn analytics_top_domains(
        &self,
        hours: u32,
        limit: u32,
    ) -> Result<AnalyticsResponse<TopDomainData>> {
        let url = format!(
            "{}/analytics/v0/pipes/top_domains.json?hours={}&limit={}",
            self.base_url, hours, limit
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics query failed")
        }
    }

    async fn analytics_domain_stats(
        &self,
        domain: &str,
        hours: u32,
    ) -> Result<AnalyticsResponse<TopDomainData>> {
        let url = format!(
            "{}/analytics/v0/pipes/domain_stats.json?domain={}&hours={}",
            self.base_url,
            urlencoding::encode(domain),
            hours
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics query failed")
        }
    }

    async fn analytics_hourly(&self, hours: u32) -> Result<AnalyticsResponse<HourlyStatsData>> {
        let url = format!(
            "{}/analytics/v0/pipes/hourly_stats.json?hours={}",
            self.base_url, hours
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics query failed")
        }
    }

    async fn analytics_error_dist(&self, hours: u32) -> Result<AnalyticsResponse<ErrorDistData>> {
        let url = format!(
            "{}/analytics/v0/pipes/error_distribution.json?hours={}",
            self.base_url, hours
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics query failed")
        }
    }

    async fn analytics_job_stats(&self, job_id: &str) -> Result<AnalyticsResponse<JobStatsData>> {
        let url = format!(
            "{}/analytics/v0/pipes/job_stats.json?job_id={}",
            self.base_url, job_id
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to API server")?;

        if response.status().is_success() {
            response.json().await.context("Failed to parse response")
        } else {
            anyhow::bail!("Analytics query failed")
        }
    }
}

// ============================================================================
// Output Helpers
// ============================================================================

fn print_job_status(job: &JobStatusResponse, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(job).unwrap());
        }
        OutputFormat::Text => {
            let status_color = match job.status.as_str() {
                "running" => "yellow",
                "completed" => "green",
                "failed" | "cancelled" => "red",
                _ => "white",
            };

            println!();
            println!("{}", "Job Status".bold().underline());
            println!();
            println!("  {} {}", "Job ID:".dimmed(), job.job_id);
            println!(
                "  {} {}",
                "Status:".dimmed(),
                job.status.color(status_color).bold()
            );
            println!("  {} {}", "Index:".dimmed(), job.index_uid);
            println!();
            println!("{}", "Progress".bold());
            println!("  {} {}", "Pages Crawled:".dimmed(), job.pages_crawled);
            println!("  {} {}", "Pages Indexed:".dimmed(), job.pages_indexed);
            println!("  {} {}", "Documents Sent:".dimmed(), job.documents_sent);
            println!(
                "  {} {}",
                "Errors:".dimmed(),
                if job.errors > 0 {
                    job.errors.to_string().red().to_string()
                } else {
                    job.errors.to_string()
                }
            );
            println!("  {} {:.2}/s", "Crawl Rate:".dimmed(), job.crawl_rate);

            if let Some(eta) = job.eta_seconds {
                println!("  {} {}s", "ETA:".dimmed(), eta);
            }

            println!();
            println!("{}", "Timing".bold());
            if let Some(ref started) = job.started_at {
                println!("  {} {}", "Started:".dimmed(), started);
            }
            if let Some(ref completed) = job.completed_at {
                println!("  {} {}", "Completed:".dimmed(), completed);
            }
            if let Some(duration) = job.duration_seconds {
                println!("  {} {}s", "Duration:".dimmed(), duration);
            }

            if let Some(ref error) = job.error_message {
                println!();
                println!("{}", "Error".bold().red());
                println!("  {}", error.red());
            }
            println!();
        }
    }
}

fn print_success(message: &str) {
    println!("{} {}", "✓".green().bold(), message);
}

fn print_error(message: &str) {
    eprintln!("{} {}", "✗".red().bold(), message);
}

fn print_info(message: &str) {
    println!("{} {}", "ℹ".blue().bold(), message);
}

fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner
}

// ============================================================================
// Command Handlers
// ============================================================================

async fn handle_crawl(
    client: &ApiClient,
    config_path: Option<String>,
    config_json: Option<String>,
    sync: bool,
    follow: bool,
    format: OutputFormat,
) -> Result<()> {
    // Parse configuration
    let config: CrawlConfig = if let Some(path) = config_path {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path))?
    } else if let Some(json) = config_json {
        serde_json::from_str(&json).context("Failed to parse inline config")?
    } else {
        anyhow::bail!("Either --config-path (-p) or --config (-c) is required");
    };

    // Validate config
    if config.start_urls.is_empty() {
        anyhow::bail!("Configuration must include at least one start_url");
    }
    if config.index_uid.is_empty() {
        anyhow::bail!("Configuration must include index_uid");
    }

    if format == OutputFormat::Text {
        print_info(&format!(
            "Starting crawl job for {} URLs targeting index '{}'",
            config.start_urls.len(),
            config.index_uid
        ));
    }

    let spinner = if format == OutputFormat::Text && sync {
        Some(create_spinner("Waiting for crawl to complete..."))
    } else if format == OutputFormat::Text {
        Some(create_spinner("Submitting crawl job..."))
    } else {
        None
    };

    // Submit job
    let response = client.create_crawl(&config, sync).await?;

    if let Some(sp) = spinner {
        sp.finish_and_clear();
    }

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response).unwrap());
        }
        OutputFormat::Text => {
            print_success(&format!("Job created: {}", response.job_id.cyan()));
            println!("  {} {}", "Status:".dimmed(), response.status);
            println!("  {} {}", "Index:".dimmed(), response.index_uid);
            println!("  {} {}", "URLs:".dimmed(), response.start_urls_count);
            println!();

            if !sync && follow {
                print_info("Following job events (Ctrl+C to stop)...");
                println!();
                handle_events(client, &response.job_id, format).await?;
            } else if !sync {
                println!(
                    "{}",
                    format!("Run 'scrapix status {}' to check progress", response.job_id).dimmed()
                );
                println!(
                    "{}",
                    format!("Run 'scrapix events {}' to stream events", response.job_id).dimmed()
                );
            }
        }
    }

    Ok(())
}

async fn handle_status(
    client: &ApiClient,
    job_id: &str,
    watch: bool,
    interval: u64,
    format: OutputFormat,
) -> Result<()> {
    if watch && format == OutputFormat::Text {
        // Watch mode - continuously poll
        loop {
            // Clear screen for fresh output
            print!("\x1B[2J\x1B[1;1H");

            let status = client.get_status(job_id).await?;
            print_job_status(&status, format);

            // Check if job is finished
            if matches!(status.status.as_str(), "completed" | "failed" | "cancelled") {
                break;
            }

            println!(
                "{}",
                format!("Refreshing every {}s... (Ctrl+C to stop)", interval).dimmed()
            );
            tokio::time::sleep(Duration::from_secs(interval)).await;
        }
    } else {
        let status = client.get_status(job_id).await?;
        print_job_status(&status, format);
    }

    Ok(())
}

async fn handle_events(client: &ApiClient, job_id: &str, format: OutputFormat) -> Result<()> {
    use futures::StreamExt;

    let stream = client.stream_events(job_id).await?;
    let mut stream = pin!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(data) => {
                // Parse SSE data lines
                for line in data.lines() {
                    if line.starts_with("data:") {
                        let json_str = line.trim_start_matches("data:").trim();
                        if !json_str.is_empty() {
                            match format {
                                OutputFormat::Json => {
                                    println!("{}", json_str);
                                }
                                OutputFormat::Text => {
                                    // Try to parse and pretty print
                                    if let Ok(event) =
                                        serde_json::from_str::<serde_json::Value>(json_str)
                                    {
                                        print_event(&event);
                                    } else {
                                        println!("{}", json_str);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                print_error(&format!("Stream error: {}", e));
                break;
            }
        }
    }

    Ok(())
}

fn print_event(event: &serde_json::Value) {
    let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();

    if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
        let icon = match event_type {
            "PageCrawled" => "📄",
            "PageFailed" => "❌",
            "UrlsDiscovered" => "🔗",
            "JobStarted" => "🚀",
            "JobCompleted" => "✅",
            "JobFailed" => "💥",
            _ => "📌",
        };

        let message = match event_type {
            "PageCrawled" => {
                let url = event
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let status = event.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("Crawled {} ({})", url.cyan(), status)
            }
            "PageFailed" => {
                let url = event
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let error = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                format!("Failed {} - {}", url.red(), error)
            }
            "UrlsDiscovered" => {
                let count = event.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                let source = event
                    .get("source_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!(
                    "Discovered {} URLs from {}",
                    count.to_string().green(),
                    source
                )
            }
            "JobStarted" => {
                let job_id = event
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!("Job {} started", job_id.cyan())
            }
            "JobCompleted" => {
                let pages = event
                    .get("pages_crawled")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                format!(
                    "Job completed - {} pages crawled",
                    pages.to_string().green()
                )
            }
            "JobFailed" => {
                let error = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                format!("Job failed: {}", error.red())
            }
            _ => format!("{:?}", event),
        };

        println!("{} {} {}", timestamp.dimmed(), icon, message);
    } else {
        println!("{} {:?}", timestamp.dimmed(), event);
    }
}

async fn handle_jobs(
    client: &ApiClient,
    limit: usize,
    offset: usize,
    format: OutputFormat,
) -> Result<()> {
    let jobs = client.list_jobs(limit, offset).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&jobs).unwrap());
        }
        OutputFormat::Text => {
            if jobs.is_empty() {
                print_info("No jobs found");
                return Ok(());
            }

            let rows: Vec<JobRow> = jobs.into_iter().map(JobRow::from).collect();
            let table = Table::new(rows).to_string();
            println!();
            println!("{}", table);
            println!();
        }
    }

    Ok(())
}

async fn handle_cancel(client: &ApiClient, job_id: &str, format: OutputFormat) -> Result<()> {
    let status = client.cancel_job(job_id).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&status).unwrap());
        }
        OutputFormat::Text => {
            print_success(&format!("Job {} cancelled", job_id.cyan()));
        }
    }

    Ok(())
}

async fn handle_health(client: &ApiClient, format: OutputFormat) -> Result<()> {
    let health = client.health().await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&health).unwrap());
        }
        OutputFormat::Text => {
            println!();
            println!("{}", "API Server Health".bold().underline());
            println!();
            println!(
                "  {} {}",
                "Status:".dimmed(),
                if health.status == "ok" {
                    health.status.green()
                } else {
                    health.status.red()
                }
            );
            println!("  {} {}", "Version:".dimmed(), health.version);
            println!(
                "  {} {}",
                "Kafka:".dimmed(),
                if health.kafka_connected {
                    "connected".green()
                } else {
                    "disconnected".red()
                }
            );
            println!();
        }
    }

    Ok(())
}

async fn handle_validate(config_path: &str, verbose: bool, format: OutputFormat) -> Result<()> {
    let content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path))?;

    let config: CrawlConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", config_path))?;

    // Validate configuration
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Required fields
    if config.start_urls.is_empty() {
        errors.push("start_urls is empty - at least one URL is required".to_string());
    }
    if config.index_uid.is_empty() {
        errors.push("index_uid is empty - required for indexing".to_string());
    }

    // Validate URLs
    for (i, url) in config.start_urls.iter().enumerate() {
        if url::Url::parse(url).is_err() {
            errors.push(format!("start_urls[{}]: invalid URL '{}'", i, url));
        }
    }

    // Check for potential issues
    if config.max_depth.is_none() && config.max_pages.is_none() {
        warnings.push("No max_depth or max_pages set - crawl may run indefinitely".to_string());
    }

    if let Some(depth) = config.max_depth {
        if depth > 10 {
            warnings.push(format!(
                "max_depth={} is quite deep - consider a smaller value",
                depth
            ));
        }
    }

    match format {
        OutputFormat::Json => {
            let result = serde_json::json!({
                "valid": errors.is_empty(),
                "errors": errors,
                "warnings": warnings,
                "config": if verbose { Some(&config) } else { None }
            });
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        OutputFormat::Text => {
            println!();
            if errors.is_empty() {
                print_success(&format!("Configuration '{}' is valid", config_path));
            } else {
                print_error(&format!(
                    "Configuration '{}' has {} error(s)",
                    config_path,
                    errors.len()
                ));
            }
            println!();

            if !errors.is_empty() {
                println!("{}", "Errors:".bold().red());
                for error in &errors {
                    println!("  {} {}", "✗".red(), error);
                }
                println!();
            }

            if !warnings.is_empty() {
                println!("{}", "Warnings:".bold().yellow());
                for warning in &warnings {
                    println!("  {} {}", "⚠".yellow(), warning);
                }
                println!();
            }

            if verbose {
                println!("{}", "Configuration Details:".bold());
                println!("  {} {}", "Index UID:".dimmed(), config.index_uid);
                println!("  {} {}", "Start URLs:".dimmed(), config.start_urls.len());
                println!("  {} {:?}", "Crawler Type:".dimmed(), config.crawler_type);
                if let Some(depth) = config.max_depth {
                    println!("  {} {}", "Max Depth:".dimmed(), depth);
                }
                if let Some(pages) = config.max_pages {
                    println!("  {} {}", "Max Pages:".dimmed(), pages);
                }
                println!();
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Configuration validation failed")
    }
}

/// Local crawl result document
#[derive(Debug, Clone, Serialize)]
struct LocalCrawlDocument {
    url: String,
    title: Option<String>,
    description: Option<String>,
    content: String,
    markdown: Option<String>,
    crawled_at: String,
    status_code: u16,
    depth: u32,
}

/// Local crawl result
#[derive(Debug, Serialize)]
struct LocalCrawlResult {
    index_uid: String,
    pages_crawled: u64,
    pages_failed: u64,
    duration_seconds: f64,
    documents: Vec<LocalCrawlDocument>,
}

async fn handle_local(
    config_path: Option<String>,
    config_json: Option<String>,
    output: Option<String>,
    concurrency: usize,
    verbose: bool,
    format: OutputFormat,
) -> Result<()> {
    // Initialize tracing if verbose
    if verbose {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("scrapix=debug,info")
            .try_init();
    }

    // Parse configuration
    let config: CrawlConfig = if let Some(path) = config_path {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path))?
    } else if let Some(json) = config_json {
        serde_json::from_str(&json).context("Failed to parse inline config")?
    } else {
        anyhow::bail!("Either --config-path (-p) or --config (-c) is required");
    };

    // Validate config
    if config.start_urls.is_empty() {
        anyhow::bail!("Configuration must include at least one start_url");
    }

    if format == OutputFormat::Text {
        print_info(&format!(
            "Starting local crawl of {} URL(s)",
            config.start_urls.len()
        ));
        println!();
    }

    let start_time = std::time::Instant::now();

    // Create HTTP client
    let http_client = reqwest::Client::builder()
        .user_agent("Scrapix/1.0 (Local Crawl)")
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    // Create feature extractor with all features
    let feature_extractor = Arc::new(Extractor::with_all_features());

    // Crawl state
    let visited: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let queue: Arc<Mutex<VecDeque<(String, u32)>>> = Arc::new(Mutex::new(VecDeque::new()));
    let documents: Arc<Mutex<Vec<LocalCrawlDocument>>> = Arc::new(Mutex::new(Vec::new()));
    let pages_crawled = Arc::new(AtomicU64::new(0));
    let pages_failed = Arc::new(AtomicU64::new(0));

    // Seed the queue
    {
        let mut q = queue.lock().await;
        let mut v = visited.lock().await;
        for url in &config.start_urls {
            if let Ok(parsed) = url::Url::parse(url) {
                let normalized = parsed.to_string();
                if v.insert(normalized.clone()) {
                    q.push_back((normalized, 0));
                }
            }
        }
    }

    // Get max depth and max pages
    let max_depth = config.max_depth.unwrap_or(2);
    let max_pages = config.max_pages.unwrap_or(100);

    // Get base domain for limiting scope
    let base_domains: HashSet<String> = config
        .start_urls
        .iter()
        .filter_map(|u| url::Url::parse(u).ok())
        .filter_map(|u| u.host_str().map(|h| h.to_string()))
        .collect();

    // Create progress bar
    let progress = if format == OutputFormat::Text {
        let pb = ProgressBar::new(max_pages);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} pages ({msg})")
                .unwrap()
                .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    // Crawl loop
    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

    loop {
        // Check if we've reached limits
        let current_pages = pages_crawled.load(Ordering::Relaxed);
        if current_pages >= max_pages {
            break;
        }

        // Get next URL from queue
        let next = {
            let mut q = queue.lock().await;
            q.pop_front()
        };

        let Some((url, depth)) = next else {
            // Queue is empty, wait a bit for in-flight requests
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Check if queue is still empty
            let q = queue.lock().await;
            if q.is_empty() {
                break;
            }
            continue;
        };

        // Skip if too deep
        if depth > max_depth {
            continue;
        }

        // Acquire semaphore
        let permit = semaphore.clone().acquire_owned().await?;

        // Clone for the task
        let http_client = http_client.clone();
        let feature_extractor = feature_extractor.clone();
        let visited = visited.clone();
        let queue = queue.clone();
        let documents = documents.clone();
        let pages_crawled = pages_crawled.clone();
        let pages_failed = pages_failed.clone();
        let base_domains = base_domains.clone();
        let progress = progress.clone();

        tokio::spawn(async move {
            let _permit = permit;

            debug!(url = %url, depth, "Fetching");

            match http_client.get(&url).send().await {
                Ok(response) => {
                    let status_code = response.status().as_u16();

                    match response.text().await {
                        Ok(html) => {
                            // Extract content using readability
                            let content = extract_content(&html);

                            // Extract features (metadata, etc.)
                            let features = feature_extractor.extract(&html).ok();

                            // Convert to markdown
                            let markdown = html_to_markdown(&html);

                            // Get title and description from features
                            let title = features
                                .as_ref()
                                .and_then(|f| f.metadata.as_ref())
                                .and_then(|m| m.title.clone());
                            let description = features
                                .as_ref()
                                .and_then(|f| f.metadata.as_ref())
                                .and_then(|m| m.description.clone());

                            // Create document
                            let doc = LocalCrawlDocument {
                                url: url.clone(),
                                title,
                                description,
                                content,
                                markdown: Some(markdown),
                                crawled_at: chrono::Utc::now().to_rfc3339(),
                                status_code,
                                depth,
                            };

                            documents.lock().await.push(doc);
                            pages_crawled.fetch_add(1, Ordering::Relaxed);

                            // Extract URLs from HTML (sync) then queue them (async)
                            let new_urls = extract_urls_from_html(&html, &url, &base_domains);
                            queue_urls(new_urls, depth, &visited, &queue).await;

                            if let Some(ref pb) = progress {
                                pb.set_position(pages_crawled.load(Ordering::Relaxed));
                                pb.set_message(format!(
                                    "{} errors",
                                    pages_failed.load(Ordering::Relaxed)
                                ));
                            }
                        }
                        Err(e) => {
                            debug!(url = %url, error = %e, "Failed to read response body");
                            pages_failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(e) => {
                    debug!(url = %url, error = %e, "Fetch failed");
                    pages_failed.fetch_add(1, Ordering::Relaxed);

                    if let Some(ref pb) = progress {
                        pb.set_message(format!("{} errors", pages_failed.load(Ordering::Relaxed)));
                    }
                }
            }
        });
    }

    // Wait for all tasks to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    if let Some(pb) = progress {
        pb.finish_with_message("done");
    }

    let duration = start_time.elapsed();

    // Build result
    let docs = documents.lock().await.clone();
    let result = LocalCrawlResult {
        index_uid: config.index_uid.clone(),
        pages_crawled: pages_crawled.load(Ordering::Relaxed),
        pages_failed: pages_failed.load(Ordering::Relaxed),
        duration_seconds: duration.as_secs_f64(),
        documents: docs,
    };

    // Output results
    if let Some(output_path) = output {
        let json = serde_json::to_string_pretty(&result)?;
        std::fs::write(&output_path, json)?;

        if format == OutputFormat::Text {
            println!();
            print_success(&format!("Results written to {}", output_path.cyan()));
        }
    }

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Text => {
            println!();
            println!("{}", "Crawl Summary".bold().underline());
            println!();
            println!(
                "  {} {}",
                "Pages Crawled:".dimmed(),
                result.pages_crawled.to_string().green()
            );
            println!(
                "  {} {}",
                "Pages Failed:".dimmed(),
                if result.pages_failed > 0 {
                    result.pages_failed.to_string().red().to_string()
                } else {
                    result.pages_failed.to_string()
                }
            );
            println!("  {} {:.2}s", "Duration:".dimmed(), result.duration_seconds);
            println!(
                "  {} {:.2}/s",
                "Rate:".dimmed(),
                result.pages_crawled as f64 / result.duration_seconds
            );
            println!();
        }
    }

    Ok(())
}

/// Extract URLs from HTML (synchronous part)
fn extract_urls_from_html(
    html: &str,
    base_url: &str,
    base_domains: &HashSet<String>,
) -> Vec<String> {
    use scraper::{Html, Selector};

    let Ok(base) = url::Url::parse(base_url) else {
        return vec![];
    };

    let document = Html::parse_document(html);
    let Ok(selector) = Selector::parse("a[href]") else {
        return vec![];
    };

    let mut urls = Vec::new();
    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            // Resolve relative URLs
            if let Ok(resolved) = base.join(href) {
                // Only include links within base domains
                if let Some(host) = resolved.host_str() {
                    if base_domains.contains(host) {
                        urls.push(resolved.to_string());
                    }
                }
            }
        }
    }
    urls
}

/// Add extracted URLs to the crawl queue
async fn queue_urls(
    urls: Vec<String>,
    depth: u32,
    visited: &Arc<Mutex<HashSet<String>>>,
    queue: &Arc<Mutex<VecDeque<(String, u32)>>>,
) {
    let mut q = queue.lock().await;
    let mut v = visited.lock().await;

    for url_str in urls {
        if v.insert(url_str.clone()) {
            q.push_back((url_str, depth + 1));
        }
    }
}

// ============================================================================
// Diagnostic Command Handlers
// ============================================================================

async fn handle_stats(client: &ApiClient, _verbose: bool, format: OutputFormat) -> Result<()> {
    let stats = client.get_stats().await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&stats).unwrap());
        }
        OutputFormat::Text => {
            println!();
            println!("{}", "System Statistics".bold().underline());
            println!();

            // Meilisearch section
            println!("{}", "Meilisearch".bold());
            if let Some(ms) = &stats.meilisearch {
                println!(
                    "  {} {}",
                    "Status:".dimmed(),
                    if ms.available {
                        "connected".green()
                    } else {
                        "unavailable".red()
                    }
                );
                println!("  {} {}", "URL:".dimmed(), ms.url);
            } else {
                println!("  {} {}", "Status:".dimmed(), "not configured".yellow());
            }

            // Jobs section
            println!();
            println!("{}", "Jobs".bold());
            println!(
                "  {} {} total ({} running, {} completed, {} failed, {} pending)",
                "Summary:".dimmed(),
                stats.jobs.total,
                stats.jobs.running.to_string().yellow(),
                stats.jobs.completed.to_string().green(),
                stats.jobs.failed.to_string().red(),
                stats.jobs.pending
            );

            // Diagnostics section
            println!();
            println!("{}", "Diagnostics".bold());
            println!(
                "  {} {}",
                "Tracked Domains:".dimmed(),
                stats.diagnostics.tracked_domains
            );
            println!(
                "  {} {}",
                "Total Requests:".dimmed(),
                stats.diagnostics.total_requests
            );
            let success_rate = if stats.diagnostics.total_requests > 0 {
                (stats.diagnostics.total_successes as f64 / stats.diagnostics.total_requests as f64
                    * 100.0) as u32
            } else {
                0
            };
            println!(
                "  {} {}% ({} successes, {} failures)",
                "Success Rate:".dimmed(),
                success_rate,
                stats.diagnostics.total_successes.to_string().green(),
                stats.diagnostics.total_failures.to_string().red()
            );
            println!(
                "  {} {}",
                "Recent Errors:".dimmed(),
                stats.diagnostics.recent_errors_count
            );
            println!();
            println!(
                "{}",
                format!("Collected at: {}", stats.collected_at).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_errors_cmd(
    client: &ApiClient,
    last: usize,
    job_id: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let errors = client.get_errors(last, job_id.as_deref()).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&errors).unwrap());
        }
        OutputFormat::Text => {
            if errors.errors.is_empty() {
                print_info("No errors found");
                return Ok(());
            }

            println!();
            println!(
                "{} ({} total)",
                "Recent Errors".bold().underline(),
                errors.total_count
            );
            println!();

            // Status code distribution
            if !errors.by_status.is_empty() {
                println!("{}", "By Status Code:".bold());
                let mut codes: Vec<_> = errors.by_status.iter().collect();
                codes.sort_by_key(|(k, _)| k.parse::<u16>().unwrap_or(0));
                for (code, count) in codes {
                    let color = if code.starts_with('5') {
                        "red"
                    } else {
                        "yellow"
                    };
                    println!("  {} {}", code.color(color), count);
                }
                println!();
            }

            // Domain distribution
            if !errors.by_domain.is_empty() {
                println!("{}", "Top Domains:".bold());
                for (domain, count) in errors.by_domain.iter().take(5) {
                    println!("  {} {}", domain.cyan(), count);
                }
                println!();
            }

            // Error list
            println!("{}", "Errors:".bold());
            for err in &errors.errors {
                let timestamp = if err.timestamp.len() > 19 {
                    &err.timestamp[11..19]
                } else {
                    &err.timestamp
                };
                let status = err
                    .status_code
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "---".to_string());

                println!(
                    "{} {} {} {}",
                    timestamp.dimmed(),
                    status.red(),
                    err.domain.cyan(),
                    truncate_url(&err.url, 50)
                );
                println!("  {} {}", "Error:".dimmed(), err.error);
            }

            println!();
            println!(
                "{}",
                format!("Source: {} (recent only)", errors.source).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_domains_cmd(
    client: &ApiClient,
    top: usize,
    filter: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let domains = client.get_domains(top, filter.as_deref()).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&domains).unwrap());
        }
        OutputFormat::Text => {
            if domains.domains.is_empty() {
                print_info("No domain data found");
                return Ok(());
            }

            println!();
            println!(
                "{} (top {} of {})",
                "Domain Statistics".bold().underline(),
                domains.domains.len(),
                domains.total_domains
            );
            println!();

            let rows: Vec<DomainRow> = domains
                .domains
                .iter()
                .map(|d| {
                    let success_rate = if d.total_requests > 0 {
                        (d.successful_requests as f64 / d.total_requests as f64 * 100.0) as u32
                    } else {
                        0
                    };

                    DomainRow {
                        domain: if d.domain.len() > 30 {
                            format!("{}...", &d.domain[..27])
                        } else {
                            d.domain.clone()
                        },
                        requests: d.total_requests,
                        success: format!("{}%", success_rate),
                        failed: d.failed_requests,
                        avg_time: d
                            .avg_response_time_ms
                            .map(|t| format!("{:.0}ms", t))
                            .unwrap_or_else(|| "-".to_string()),
                    }
                })
                .collect();

            let table = Table::new(rows).to_string();
            println!("{}", table);

            println!();
            println!("{}", format!("Source: {}", domains.source).dimmed());
            println!();
        }
    }
    Ok(())
}

fn truncate_url(url: &str, max_len: usize) -> String {
    if url.len() <= max_len {
        url.to_string()
    } else {
        format!("{}...", &url[..max_len - 3])
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}

// ============================================================================
// Analytics Command Handlers
// ============================================================================

async fn handle_analytics(
    client: &ApiClient,
    cmd: AnalyticsCommands,
    format: OutputFormat,
) -> Result<()> {
    match cmd {
        AnalyticsCommands::Pipes => handle_analytics_pipes(client, format).await,
        AnalyticsCommands::Kpis { hours } => handle_analytics_kpis(client, hours, format).await,
        AnalyticsCommands::TopDomains { hours, limit } => {
            handle_analytics_top_domains(client, hours, limit, format).await
        }
        AnalyticsCommands::DomainStats { domain, hours } => {
            handle_analytics_domain_stats(client, &domain, hours, format).await
        }
        AnalyticsCommands::Hourly { hours } => handle_analytics_hourly(client, hours, format).await,
        AnalyticsCommands::ErrorDist { hours } => {
            handle_analytics_error_dist(client, hours, format).await
        }
        AnalyticsCommands::JobStats { job_id } => {
            handle_analytics_job_stats(client, &job_id, format).await
        }
    }
}

async fn handle_analytics_pipes(client: &ApiClient, format: OutputFormat) -> Result<()> {
    let pipes = client.analytics_pipes().await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&pipes)?);
        }
        OutputFormat::Text => {
            println!();
            println!("{}", "Available Analytics Pipes".bold().underline());
            println!();
            for pipe in &pipes {
                println!("  {} - {}", pipe.name.cyan(), pipe.description);
                println!("    {}", pipe.endpoint.dimmed());
            }
            println!();
        }
    }
    Ok(())
}

async fn handle_analytics_kpis(client: &ApiClient, hours: u32, format: OutputFormat) -> Result<()> {
    let response = client.analytics_kpis(hours).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Text => {
            if response.data.is_empty() {
                print_info("No data available");
                return Ok(());
            }

            let kpis = &response.data[0];

            println!();
            println!(
                "{} (last {} hours)",
                "Key Performance Indicators".bold().underline(),
                hours
            );
            println!();
            println!("  {} {}", "Total Crawls:".dimmed(), kpis.total_crawls);
            println!(
                "  {} {}",
                "Total Bytes:".dimmed(),
                format_bytes(kpis.total_bytes)
            );
            println!("  {} {}", "Unique Domains:".dimmed(), kpis.unique_domains);
            println!("  {} {:.1}%", "Success Rate:".dimmed(), kpis.success_rate);
            println!(
                "  {} {:.0}ms",
                "Avg Response Time:".dimmed(),
                kpis.avg_response_time_ms
            );
            println!(
                "  {} {}",
                "Errors:".dimmed(),
                kpis.errors_count.to_string().red()
            );
            println!();
            println!(
                "{}",
                format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_analytics_top_domains(
    client: &ApiClient,
    hours: u32,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    let response = client.analytics_top_domains(hours, limit).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Text => {
            if response.data.is_empty() {
                print_info("No data available");
                return Ok(());
            }

            println!();
            println!(
                "{} (last {} hours)",
                "Top Domains".bold().underline(),
                hours
            );
            println!();

            let rows: Vec<TopDomainAnalyticsRow> = response
                .data
                .iter()
                .map(|d| TopDomainAnalyticsRow {
                    domain: if d.domain.len() > 35 {
                        format!("{}...", &d.domain[..32])
                    } else {
                        d.domain.clone()
                    },
                    requests: d.total_requests,
                    success: format!("{:.1}%", d.success_rate),
                    failed: d.failed_requests,
                    avg_time: format!("{:.0}ms", d.avg_response_time_ms),
                    bytes: format_bytes(d.total_bytes),
                })
                .collect();

            let table = Table::new(rows).to_string();
            println!("{}", table);

            println!();
            println!(
                "{}",
                format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_analytics_domain_stats(
    client: &ApiClient,
    domain: &str,
    hours: u32,
    format: OutputFormat,
) -> Result<()> {
    let response = client.analytics_domain_stats(domain, hours).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Text => {
            if response.data.is_empty() {
                print_info(&format!("No data for domain: {}", domain));
                return Ok(());
            }

            let d = &response.data[0];

            println!();
            println!(
                "{}: {} (last {} hours)",
                "Domain Statistics".bold().underline(),
                d.domain.cyan(),
                hours
            );
            println!();
            println!("  {} {}", "Total Requests:".dimmed(), d.total_requests);
            println!(
                "  {} {}",
                "Successful:".dimmed(),
                d.successful_requests.to_string().green()
            );
            println!(
                "  {} {}",
                "Failed:".dimmed(),
                d.failed_requests.to_string().red()
            );
            println!("  {} {:.1}%", "Success Rate:".dimmed(), d.success_rate);
            println!(
                "  {} {:.0}ms",
                "Avg Response Time:".dimmed(),
                d.avg_response_time_ms
            );
            println!(
                "  {} {}",
                "Total Bytes:".dimmed(),
                format_bytes(d.total_bytes)
            );
            println!("  {} {}", "Unique URLs:".dimmed(), d.unique_urls);
            println!();
            println!(
                "{}",
                format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_analytics_hourly(
    client: &ApiClient,
    hours: u32,
    format: OutputFormat,
) -> Result<()> {
    let response = client.analytics_hourly(hours).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Text => {
            if response.data.is_empty() {
                print_info("No data available");
                return Ok(());
            }

            println!();
            println!(
                "{} (last {} hours)",
                "Hourly Statistics".bold().underline(),
                hours
            );
            println!();

            let rows: Vec<HourlyRow> = response
                .data
                .iter()
                .map(|h| {
                    // Extract just the time portion if it's a full datetime
                    let hour_display = if h.hour.len() > 16 {
                        h.hour[11..16].to_string()
                    } else {
                        h.hour.clone()
                    };
                    HourlyRow {
                        hour: hour_display,
                        requests: h.requests,
                        success: format!("{:.1}%", h.success_rate),
                        failed: h.failures,
                        avg_time: format!("{:.0}ms", h.avg_response_time_ms),
                    }
                })
                .collect();

            let table = Table::new(rows).to_string();
            println!("{}", table);

            println!();
            println!(
                "{}",
                format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_analytics_error_dist(
    client: &ApiClient,
    hours: u32,
    format: OutputFormat,
) -> Result<()> {
    let response = client.analytics_error_dist(hours).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Text => {
            if response.data.is_empty() {
                print_info("No errors found");
                return Ok(());
            }

            println!();
            println!(
                "{} (last {} hours)",
                "Error Distribution".bold().underline(),
                hours
            );
            println!();

            let rows: Vec<ErrorDistRow> = response
                .data
                .iter()
                .map(|e| ErrorDistRow {
                    status: e.status_code,
                    count: e.count,
                    percentage: format!("{:.1}%", e.percentage),
                })
                .collect();

            let table = Table::new(rows).to_string();
            println!("{}", table);

            println!();
            println!(
                "{}",
                format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

async fn handle_analytics_job_stats(
    client: &ApiClient,
    job_id: &str,
    format: OutputFormat,
) -> Result<()> {
    let response = client.analytics_job_stats(job_id).await?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        OutputFormat::Text => {
            if response.data.is_empty() {
                print_info(&format!("No data for job: {}", job_id));
                return Ok(());
            }

            let j = &response.data[0];

            println!();
            println!(
                "{}: {}",
                "Job Statistics".bold().underline(),
                j.job_id.cyan()
            );
            println!();
            println!("  {} {}", "Total URLs:".dimmed(), j.total_urls);
            println!(
                "  {} {}",
                "Successful:".dimmed(),
                j.successful_urls.to_string().green()
            );
            println!(
                "  {} {}",
                "Failed:".dimmed(),
                j.failed_urls.to_string().red()
            );
            println!("  {} {:.1}%", "Success Rate:".dimmed(), j.success_rate);
            println!(
                "  {} {}",
                "Total Bytes:".dimmed(),
                format_bytes(j.total_bytes)
            );
            println!(
                "  {} {:.0}ms",
                "Avg Response Time:".dimmed(),
                j.avg_response_time_ms
            );
            println!("  {} {}", "Unique Domains:".dimmed(), j.unique_domains);
            println!("  {} {}", "Started At:".dimmed(), j.started_at);
            println!("  {} {}", "Last Activity:".dimmed(), j.last_activity_at);
            println!("  {} {}s", "Duration:".dimmed(), j.duration_seconds);
            println!();
            println!(
                "{}",
                format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
            );
            println!();
        }
    }
    Ok(())
}

// ============================================================================
// Benchmark Command Handlers
// ============================================================================

fn handle_bench(cmd: BenchCommands, format: OutputFormat) -> Result<()> {
    match cmd {
        BenchCommands::All {
            output,
            iterations,
            verbose,
        } => run_benchmarks(
            &["wikipedia_e2e", "integrated_benchmarks"],
            &output,
            iterations,
            verbose,
            format,
        ),
        BenchCommands::Wikipedia {
            output,
            iterations,
            verbose,
        } => run_benchmarks(&["wikipedia_e2e"], &output, iterations, verbose, format),
        BenchCommands::Integrated {
            output,
            iterations,
            verbose,
        } => run_benchmarks(
            &["integrated_benchmarks"],
            &output,
            iterations,
            verbose,
            format,
        ),
        BenchCommands::Parser { output, verbose } => {
            run_benchmarks(&["integrated_benchmarks"], &output, 1, verbose, format)
        }
    }
}

fn run_benchmarks(
    benches: &[&str],
    output_dir: &str,
    iterations: u32,
    verbose: bool,
    format: OutputFormat,
) -> Result<()> {
    use std::process::Command;

    if format == OutputFormat::Text {
        println!();
        println!("{}", "Scrapix Benchmarking".bold().underline());
        println!();
        print_info(&format!("Benchmarks: {}", benches.join(", ")));
        print_info(&format!("Output directory: {}", output_dir));
        print_info(&format!("Iterations: {}", iterations));
        println!();
    }

    // Create output directory
    std::fs::create_dir_all(output_dir)?;

    // Ensure release build
    if format == OutputFormat::Text {
        print_info("Ensuring release build is up to date...");
    }

    let build_status = Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .context("Failed to run cargo build")?;

    if !build_status.success() {
        anyhow::bail!("Build failed");
    }

    let total_start = std::time::Instant::now();
    let mut results: Vec<serde_json::Value> = Vec::new();

    for iteration in 1..=iterations {
        if iterations > 1 && format == OutputFormat::Text {
            println!();
            print_info(&format!(
                "=== Iteration {} of {} ===",
                iteration, iterations
            ));
        }

        for bench in benches {
            let start = std::time::Instant::now();

            if format == OutputFormat::Text {
                print_info(&format!("Running benchmark: {}", bench));
            }

            let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
            let output_file = format!("{}/{}-{}.txt", output_dir, bench, timestamp);

            let output = Command::new("cargo")
                .args(["bench", "--bench", bench])
                .output()
                .context("Failed to run cargo bench")?;

            let duration = start.elapsed();

            // Save output to file
            std::fs::write(&output_file, &output.stdout)?;

            if format == OutputFormat::Text {
                if verbose {
                    println!("{}", String::from_utf8_lossy(&output.stdout));
                }

                print_success(&format!(
                    "Benchmark '{}' completed in {:.2}s",
                    bench,
                    duration.as_secs_f64()
                ));
                print_info(&format!("Results saved to: {}", output_file));

                // Extract and show key results
                let stdout = String::from_utf8_lossy(&output.stdout);
                println!();
                println!("{}", "Key Results:".bold());
                for line in stdout.lines() {
                    if line.contains("time:") || line.contains("thrpt:") {
                        println!("  {}", line);
                    }
                }
            }

            results.push(serde_json::json!({
                "benchmark": bench,
                "iteration": iteration,
                "duration_seconds": duration.as_secs_f64(),
                "output_file": output_file,
                "success": output.status.success(),
            }));
        }
    }

    let total_duration = total_start.elapsed();

    match format {
        OutputFormat::Json => {
            let result = serde_json::json!({
                "benchmarks": benches,
                "iterations": iterations,
                "total_duration_seconds": total_duration.as_secs_f64(),
                "results": results,
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Text => {
            println!();
            print_success(&format!(
                "All benchmarks completed in {:.2}s",
                total_duration.as_secs_f64()
            ));
            println!();
        }
    }

    Ok(())
}

// ============================================================================
// Kubernetes Command Handlers
// ============================================================================

fn handle_k8s(cmd: K8sCommands, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    // Check kubectl is available
    let kubectl_check = Command::new("kubectl").args(["cluster-info"]).output();

    if kubectl_check.is_err() || !kubectl_check.as_ref().unwrap().status.success() {
        anyhow::bail!("Cannot connect to Kubernetes cluster. Check your kubeconfig.");
    }

    match cmd {
        K8sCommands::Deploy { namespace, overlay } => k8s_deploy(&namespace, &overlay, format),
        K8sCommands::Destroy {
            namespace,
            overlay,
            yes,
        } => k8s_destroy(&namespace, &overlay, yes, format),
        K8sCommands::Status { namespace, watch } => k8s_status(&namespace, watch, format),
        K8sCommands::Logs {
            component,
            namespace,
            follow,
        } => k8s_logs(&component, &namespace, follow),
        K8sCommands::Scale {
            component,
            replicas,
            namespace,
        } => k8s_scale(&component, replicas, &namespace, format),
        K8sCommands::Restart {
            component,
            namespace,
        } => k8s_restart(&component, &namespace, format),
        K8sCommands::PortForward { namespace } => k8s_port_forward(&namespace, format),
    }
}

fn k8s_deploy(namespace: &str, overlay: &str, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    let overlay_path = format!("deploy/kubernetes/overlays/{}", overlay);
    if !std::path::Path::new(&overlay_path).exists() {
        anyhow::bail!("Overlay not found: {}", overlay_path);
    }

    if format == OutputFormat::Text {
        println!();
        println!("{}", "Kubernetes Deployment".bold().underline());
        println!();
        print_info(&format!("Namespace: {}", namespace));
        print_info(&format!("Overlay: {}", overlay));
        println!();
    }

    // Create namespace
    print_info("Creating namespace...");
    let _ = Command::new("kubectl")
        .args([
            "create",
            "namespace",
            namespace,
            "--dry-run=client",
            "-o",
            "yaml",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()?
        .wait_with_output()
        .and_then(|output| {
            Command::new("kubectl")
                .args(["apply", "-f", "-"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child.stdin.as_mut().unwrap().write_all(&output.stdout)?;
                    child.wait()
                })
        });

    // Apply kustomize
    print_info("Applying kustomize overlay...");
    let status = Command::new("kubectl")
        .args(["apply", "-k", &overlay_path, "-n", namespace])
        .status()
        .context("Failed to apply kustomize")?;

    if !status.success() {
        anyhow::bail!("kubectl apply failed");
    }

    // Wait for rollout
    print_info("Waiting for deployments to be ready...");
    let _ = Command::new("kubectl")
        .args([
            "rollout",
            "status",
            "deployment",
            "-n",
            namespace,
            "--timeout=300s",
        ])
        .status();

    print_success("Deployment complete!");
    println!();

    // Show status
    k8s_status(namespace, false, format)?;

    Ok(())
}

fn k8s_destroy(namespace: &str, overlay: &str, yes: bool, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    if !yes {
        println!();
        println!(
            "{} This will delete all Scrapix resources in namespace '{}'",
            "WARNING:".yellow().bold(),
            namespace
        );
        print!("Are you sure? (y/N) ");
        use std::io::Write;
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            print_info("Cancelled");
            return Ok(());
        }
    }

    let overlay_path = format!("deploy/kubernetes/overlays/{}", overlay);

    if format == OutputFormat::Text {
        print_info("Destroying Scrapix deployment...");
    }

    let status = Command::new("kubectl")
        .args([
            "delete",
            "-k",
            &overlay_path,
            "-n",
            namespace,
            "--ignore-not-found",
        ])
        .status()
        .context("Failed to delete resources")?;

    if status.success() {
        print_success("Resources deleted");
    } else {
        print_error("Some resources may not have been deleted");
    }

    Ok(())
}

fn k8s_status(namespace: &str, watch: bool, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    if watch {
        // Use watch command
        let _ = Command::new("watch")
            .args([
                "-n",
                "2",
                "kubectl",
                "get",
                "pods,svc,deployments",
                "-n",
                namespace,
                "-o",
                "wide",
            ])
            .status();
        return Ok(());
    }

    match format {
        OutputFormat::Json => {
            let output = Command::new("kubectl")
                .args(["get", "pods,svc,deployments", "-n", namespace, "-o", "json"])
                .output()
                .context("Failed to get status")?;
            println!("{}", String::from_utf8_lossy(&output.stdout));
        }
        OutputFormat::Text => {
            println!();
            println!("{}", "Scrapix Deployment Status".bold().underline());
            println!();

            println!("{}", "Deployments:".bold());
            let _ = Command::new("kubectl")
                .args(["get", "deployments", "-n", namespace, "-o", "wide"])
                .status();
            println!();

            println!("{}", "Pods:".bold());
            let _ = Command::new("kubectl")
                .args(["get", "pods", "-n", namespace, "-o", "wide"])
                .status();
            println!();

            println!("{}", "Services:".bold());
            let _ = Command::new("kubectl")
                .args(["get", "svc", "-n", namespace])
                .status();
            println!();

            println!("{}", "Resource Usage:".bold());
            let _ = Command::new("kubectl")
                .args(["top", "pods", "-n", namespace])
                .status();
            println!();
        }
    }

    Ok(())
}

fn k8s_logs(component: &str, namespace: &str, follow: bool) -> Result<()> {
    use std::process::Command;

    let mut args = vec!["logs", "-n", namespace];

    if component == "all" {
        args.extend([
            "-l",
            "app.kubernetes.io/name=scrapix",
            "--all-containers",
            "--prefix",
        ]);
    } else {
        let deployment = match component {
            "api" => "deployment/scrapix-api",
            "frontier" => "deployment/scrapix-frontier",
            "crawler" => "deployment/scrapix-crawler",
            "content" => "deployment/scrapix-content",
            _ => return Err(anyhow::anyhow!("Unknown component: {}", component)),
        };
        args.push(deployment);
        args.push("--all-containers");
    }

    if follow {
        args.push("-f");
    }

    print_info(&format!("Streaming logs from {}...", component));

    let _ = Command::new("kubectl").args(&args).status();

    Ok(())
}

fn k8s_scale(component: &str, replicas: u32, namespace: &str, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    let deployment = match component {
        "api" => "scrapix-api",
        "frontier" => "scrapix-frontier",
        "crawler" => "scrapix-crawler",
        "content" => "scrapix-content",
        _ => return Err(anyhow::anyhow!("Unknown component: {}", component)),
    };

    if format == OutputFormat::Text {
        print_info(&format!(
            "Scaling {} to {} replicas...",
            deployment, replicas
        ));
    }

    let status = Command::new("kubectl")
        .args([
            "scale",
            "deployment",
            deployment,
            "-n",
            namespace,
            &format!("--replicas={}", replicas),
        ])
        .status()
        .context("Failed to scale deployment")?;

    if !status.success() {
        anyhow::bail!("Scale command failed");
    }

    // Wait for rollout
    let _ = Command::new("kubectl")
        .args([
            "rollout",
            "status",
            "deployment",
            deployment,
            "-n",
            namespace,
            "--timeout=120s",
        ])
        .status();

    print_success(&format!("Scaled {} to {} replicas", deployment, replicas));

    Ok(())
}

fn k8s_restart(component: &str, namespace: &str, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    if format == OutputFormat::Text {
        print_info(&format!("Restarting {}...", component));
    }

    let status = if component == "all" {
        Command::new("kubectl")
            .args([
                "rollout",
                "restart",
                "deployment",
                "-n",
                namespace,
                "-l",
                "app.kubernetes.io/name=scrapix",
            ])
            .status()
    } else {
        let deployment = match component {
            "api" => "scrapix-api",
            "frontier" => "scrapix-frontier",
            "crawler" => "scrapix-crawler",
            "content" => "scrapix-content",
            _ => return Err(anyhow::anyhow!("Unknown component: {}", component)),
        };
        Command::new("kubectl")
            .args([
                "rollout",
                "restart",
                "deployment",
                deployment,
                "-n",
                namespace,
            ])
            .status()
    };

    status.context("Failed to restart")?;

    // Wait for rollout
    let _ = Command::new("kubectl")
        .args([
            "rollout",
            "status",
            "deployment",
            "-n",
            namespace,
            "--timeout=120s",
        ])
        .status();

    print_success("Restart complete");

    Ok(())
}

fn k8s_port_forward(namespace: &str, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    if format == OutputFormat::Text {
        print_info("Setting up port forwarding...");
        println!();
    }

    // Kill existing port forwards
    let _ = Command::new("pkill")
        .args(["-f", "kubectl port-forward.*scrapix"])
        .status();

    // Start port forwards in background
    let _ = Command::new("kubectl")
        .args([
            "port-forward",
            "-n",
            namespace,
            "svc/scrapix-api",
            "8080:8080",
        ])
        .spawn();

    print_success("API Server: http://localhost:8080");

    let _ = Command::new("kubectl")
        .args([
            "port-forward",
            "-n",
            namespace,
            "svc/meilisearch",
            "7700:7700",
        ])
        .spawn();

    print_success("Meilisearch: http://localhost:7700");

    let _ = Command::new("kubectl")
        .args([
            "port-forward",
            "-n",
            namespace,
            "svc/redpanda-console",
            "8090:8080",
        ])
        .spawn();

    print_success("Redpanda Console: http://localhost:8090");

    println!();
    print_info("Port forwards active. Press Ctrl+C to stop.");

    // Wait forever
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

// ============================================================================
// Infrastructure Command Handlers
// ============================================================================

fn handle_infra(cmd: InfraCommands, format: OutputFormat) -> Result<()> {
    use std::process::Command;

    match cmd {
        InfraCommands::Up => {
            if format == OutputFormat::Text {
                print_info("Starting infrastructure...");
            }

            let status = Command::new("docker")
                .args([
                    "compose",
                    "-f",
                    "docker-compose.yml",
                    "-f",
                    "docker-compose.dev.yml",
                    "up",
                    "-d",
                ])
                .status()
                .context("Failed to start infrastructure")?;

            if status.success() {
                print_success("Infrastructure started");
                println!();
                println!("{}", "Services:".bold());
                println!("  - Redpanda (Kafka):    localhost:19092");
                println!("  - Meilisearch:         localhost:7700");
                println!("  - DragonflyDB (Redis): localhost:6380");
                println!("  - Redpanda Console:    localhost:8090");
                println!();
            } else {
                print_error("Failed to start infrastructure");
            }
        }

        InfraCommands::Down => {
            if format == OutputFormat::Text {
                print_info("Stopping infrastructure...");
            }

            let status = Command::new("docker")
                .args([
                    "compose",
                    "-f",
                    "docker-compose.yml",
                    "-f",
                    "docker-compose.dev.yml",
                    "down",
                ])
                .status()
                .context("Failed to stop infrastructure")?;

            if status.success() {
                print_success("Infrastructure stopped");
            }
        }

        InfraCommands::Restart => {
            if format == OutputFormat::Text {
                print_info("Restarting infrastructure...");
            }

            let status = Command::new("docker")
                .args([
                    "compose",
                    "-f",
                    "docker-compose.yml",
                    "-f",
                    "docker-compose.dev.yml",
                    "restart",
                ])
                .status()
                .context("Failed to restart infrastructure")?;

            if status.success() {
                print_success("Infrastructure restarted");
            }
        }

        InfraCommands::Status => {
            let _ = Command::new("docker").args(["compose", "ps"]).status();
        }

        InfraCommands::Logs { service, follow } => {
            let mut args = vec!["compose", "logs"];

            if follow {
                args.push("-f");
            }

            if let Some(ref svc) = service {
                args.push(svc);
            }

            let _ = Command::new("docker").args(&args).status();
        }

        InfraCommands::Reset { yes } => {
            if !yes {
                println!();
                println!(
                    "{} This will delete all data volumes",
                    "WARNING:".yellow().bold()
                );
                print!("Are you sure? (y/N) ");
                use std::io::Write;
                std::io::stdout().flush()?;

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    print_info("Cancelled");
                    return Ok(());
                }
            }

            let status = Command::new("docker")
                .args([
                    "compose",
                    "-f",
                    "docker-compose.yml",
                    "-f",
                    "docker-compose.dev.yml",
                    "down",
                    "-v",
                ])
                .status()
                .context("Failed to reset infrastructure")?;

            if status.success() {
                print_success("Infrastructure reset");
            }
        }
    }

    Ok(())
}

// ============================================================================
// Public entry point
// ============================================================================

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let client = ApiClient::new(&cli.api_url);

    let result = match cli.command {
        Commands::Crawl {
            config_path,
            config,
            sync,
            follow,
        } => handle_crawl(&client, config_path, config, sync, follow, cli.output).await,

        Commands::Status {
            job_id,
            watch,
            interval,
        } => handle_status(&client, &job_id, watch, interval, cli.output).await,

        Commands::Events { job_id } => handle_events(&client, &job_id, cli.output).await,

        Commands::Jobs { limit, offset } => handle_jobs(&client, limit, offset, cli.output).await,

        Commands::Cancel { job_id } => handle_cancel(&client, &job_id, cli.output).await,

        Commands::Health => handle_health(&client, cli.output).await,

        Commands::Validate {
            config_path,
            verbose,
        } => handle_validate(&config_path, verbose, cli.output).await,

        Commands::Local {
            config_path,
            config,
            output,
            concurrency,
            verbose,
        } => {
            handle_local(
                config_path,
                config,
                output,
                concurrency,
                verbose,
                cli.output,
            )
            .await
        }

        Commands::Stats { verbose } => handle_stats(&client, verbose, cli.output).await,

        Commands::Errors { last, job } => handle_errors_cmd(&client, last, job, cli.output).await,

        Commands::Domains { top, filter } => {
            handle_domains_cmd(&client, top, filter, cli.output).await
        }

        Commands::Analytics(cmd) => handle_analytics(&client, cmd, cli.output).await,

        Commands::Bench(cmd) => handle_bench(cmd, cli.output),

        Commands::K8s(cmd) => handle_k8s(cmd, cli.output),

        Commands::Infra(cmd) => handle_infra(cmd, cli.output),
    };

    if let Err(e) = result {
        print_error(&e.to_string());
        std::process::exit(1);
    }

    Ok(())
}
