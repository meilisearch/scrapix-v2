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
struct Cli {
    /// API server URL
    #[arg(
        short,
        long,
        env = "SCRAPIX_API_URL",
        default_value = "http://localhost:8080"
    )]
    api_url: String,

    /// Output format (text, json)
    #[arg(short, long, default_value = "text")]
    output: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand, Debug)]
enum Commands {
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
                println!(
                    "  {} {:?}",
                    "Crawler Type:".dimmed(),
                    config.crawler_type
                );
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
                                pb.set_message(format!("{} errors", pages_failed.load(Ordering::Relaxed)));
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
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
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
        } => handle_local(config_path, config, output, concurrency, verbose, cli.output).await,
    };

    if let Err(e) = result {
        print_error(&e.to_string());
        std::process::exit(1);
    }

    Ok(())
}
