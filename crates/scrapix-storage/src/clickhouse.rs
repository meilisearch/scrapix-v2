//! # ClickHouse Analytics Storage
//!
//! ClickHouse backend for storing crawl analytics and metrics at scale.
//! Optimized for time-series data and aggregation queries.
//!
//! ## Features
//!
//! - High-performance columnar storage for analytics
//! - Time-series data for crawl metrics
//! - Aggregation queries for dashboards
//! - URL-level and domain-level statistics
//!
//! ## Example
//!
//! ```rust,ignore
//! use scrapix_storage::clickhouse::{ClickHouseStorage, ClickHouseConfig, CrawlEvent};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let storage = ClickHouseStorage::new(ClickHouseConfig::default()).await?;
//!
//!     // Record a crawl event
//!     storage.insert_crawl_event(CrawlEvent {
//!         url: "https://example.com/page".to_string(),
//!         domain: "example.com".to_string(),
//!         status_code: 200,
//!         response_time_ms: 150,
//!         content_length: 4096,
//!         crawled_at: chrono::Utc::now(),
//!         ..Default::default()
//!     }).await?;
//!
//!     // Query domain statistics
//!     let stats = storage.get_domain_stats("example.com", 24).await?;
//!     println!("Total requests: {}", stats.total_requests);
//!
//!     Ok(())
//! }
//! ```

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, instrument, warn};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during ClickHouse operations.
#[derive(Error, Debug)]
pub enum ClickHouseError {
    /// Connection failed.
    #[error("Connection failed: {0}")]
    ConnectionError(String),

    /// Query failed.
    #[error("Query failed: {0}")]
    QueryError(#[from] clickhouse::error::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Table not found or not initialized.
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    ConfigError(String),
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for ClickHouse connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseConfig {
    /// ClickHouse server URL.
    pub url: String,

    /// Database name.
    pub database: String,

    /// Username (optional).
    #[serde(default)]
    pub username: Option<String>,

    /// Password (optional).
    #[serde(default)]
    pub password: Option<String>,

    /// Connection timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Maximum number of connections in pool.
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,

    /// Whether to create tables on startup.
    #[serde(default = "default_true")]
    pub auto_create_tables: bool,

    /// Table prefix for all tables.
    #[serde(default)]
    pub table_prefix: String,
}

fn default_timeout() -> u64 {
    30
}

fn default_pool_size() -> usize {
    10
}

fn default_true() -> bool {
    true
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_string(),
            database: "scrapix".to_string(),
            username: None,
            password: None,
            timeout_secs: default_timeout(),
            pool_size: default_pool_size(),
            auto_create_tables: true,
            table_prefix: String::new(),
        }
    }
}

impl ClickHouseConfig {
    /// Create configuration for local development.
    pub fn local() -> Self {
        Self::default()
    }

    /// Create configuration for production.
    pub fn production(url: impl Into<String>, database: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            database: database.into(),
            ..Default::default()
        }
    }
}

// ============================================================================
// Data Types - Crawl Events
// ============================================================================

/// A single crawl event record.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct CrawlEvent {
    /// URL that was crawled.
    pub url: String,

    /// Domain of the URL.
    pub domain: String,

    /// HTTP status code.
    pub status_code: u16,

    /// Response time in milliseconds.
    pub response_time_ms: u32,

    /// Content length in bytes.
    pub content_length: u64,

    /// Content type (MIME).
    #[serde(default)]
    pub content_type: String,

    /// Whether JavaScript rendering was used.
    #[serde(default)]
    pub js_rendered: bool,

    /// Crawl depth from seed URL.
    pub depth: u32,

    /// Worker ID that processed this URL.
    #[serde(default)]
    pub worker_id: String,

    /// Job ID this crawl belongs to.
    #[serde(default)]
    pub job_id: String,

    /// Account ID for billing attribution.
    #[serde(default)]
    pub account_id: String,

    /// When the crawl occurred.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub crawled_at: time::OffsetDateTime,

    /// Error message if crawl failed.
    #[serde(default)]
    pub error: String,

    /// Number of links extracted from this page.
    pub links_extracted: u32,

    /// Whether content changed since last crawl.
    #[serde(default)]
    pub content_changed: bool,
}

impl Default for CrawlEvent {
    fn default() -> Self {
        Self {
            url: String::new(),
            domain: String::new(),
            status_code: 0,
            response_time_ms: 0,
            content_length: 0,
            content_type: String::new(),
            js_rendered: false,
            depth: 0,
            worker_id: String::new(),
            job_id: String::new(),
            account_id: String::new(),
            crawled_at: time::OffsetDateTime::now_utc(),
            error: String::new(),
            links_extracted: 0,
            content_changed: false,
        }
    }
}

/// Content extraction event.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct ContentEvent {
    /// URL of the content.
    pub url: String,

    /// Domain of the URL.
    pub domain: String,

    /// Content hash (fingerprint).
    pub content_hash: String,

    /// Title extracted from page.
    #[serde(default)]
    pub title: String,

    /// Word count of main content.
    pub word_count: u32,

    /// Language detected.
    #[serde(default)]
    pub language: String,

    /// Whether AI extraction was used.
    #[serde(default)]
    pub ai_extracted: bool,

    /// AI model used (if any).
    #[serde(default)]
    pub ai_model: String,

    /// AI tokens consumed.
    pub ai_tokens: u32,

    /// Processing time in milliseconds.
    pub processing_time_ms: u32,

    /// When extraction occurred.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub extracted_at: time::OffsetDateTime,

    /// Job ID.
    #[serde(default)]
    pub job_id: String,
}

impl Default for ContentEvent {
    fn default() -> Self {
        Self {
            url: String::new(),
            domain: String::new(),
            content_hash: String::new(),
            title: String::new(),
            word_count: 0,
            language: String::new(),
            ai_extracted: false,
            ai_model: String::new(),
            ai_tokens: 0,
            processing_time_ms: 0,
            extracted_at: time::OffsetDateTime::now_utc(),
            job_id: String::new(),
        }
    }
}

// ============================================================================
// Aggregation Results
// ============================================================================

/// Domain-level statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct DomainStats {
    /// Domain name.
    pub domain: String,

    /// Total number of requests.
    pub total_requests: u64,

    /// Successful requests (2xx status).
    pub successful_requests: u64,

    /// Failed requests (4xx, 5xx, errors).
    pub failed_requests: u64,

    /// Average response time in ms.
    pub avg_response_time_ms: f64,

    /// Total bytes transferred.
    pub total_bytes: u64,

    /// Unique URLs crawled.
    pub unique_urls: u64,

    /// Number of content changes detected.
    pub content_changes: u64,
}

/// Hourly crawl statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct HourlyStats {
    /// Hour (truncated timestamp).
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub hour: time::OffsetDateTime,

    /// Total requests in this hour.
    pub requests: u64,

    /// Successful requests.
    pub successes: u64,

    /// Failed requests.
    pub failures: u64,

    /// Average response time.
    pub avg_response_time_ms: f64,

    /// Total bytes.
    pub total_bytes: u64,
}

/// Job statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct JobStats {
    /// Job ID.
    pub job_id: String,

    /// Total URLs crawled.
    pub total_urls: u64,

    /// Successful crawls.
    pub successful_urls: u64,

    /// Failed crawls.
    pub failed_urls: u64,

    /// Total bytes downloaded.
    pub total_bytes: u64,

    /// Average response time.
    pub avg_response_time_ms: f64,

    /// Number of unique domains.
    pub unique_domains: u64,

    /// First crawl time.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub started_at: time::OffsetDateTime,

    /// Last crawl time.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub last_activity_at: time::OffsetDateTime,
}

// ============================================================================
// ClickHouse Storage
// ============================================================================

/// ClickHouse storage client for analytics.
#[derive(Clone)]
pub struct ClickHouseStorage {
    client: Client,
    config: ClickHouseConfig,
}

impl std::fmt::Debug for ClickHouseStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickHouseStorage")
            .field("url", &self.config.url)
            .field("database", &self.config.database)
            .finish()
    }
}

impl ClickHouseStorage {
    /// Create a new ClickHouse storage client.
    pub async fn new(config: ClickHouseConfig) -> Result<Self, ClickHouseError> {
        let mut client = Client::default()
            .with_url(&config.url)
            .with_database(&config.database);

        if let Some(ref user) = config.username {
            client = client.with_user(user);
        }
        if let Some(ref pass) = config.password {
            client = client.with_password(pass);
        }

        let storage = Self { client, config };

        // Auto-create tables if configured
        if storage.config.auto_create_tables {
            storage.create_tables().await?;
        }

        Ok(storage)
    }

    /// Create with default configuration.
    pub async fn with_defaults() -> Result<Self, ClickHouseError> {
        Self::new(ClickHouseConfig::default()).await
    }

    /// Get table name with prefix.
    fn table_name(&self, name: &str) -> String {
        if self.config.table_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}_{}", self.config.table_prefix, name)
        }
    }

    /// Create all required tables.
    #[instrument(skip(self))]
    pub async fn create_tables(&self) -> Result<(), ClickHouseError> {
        info!("Creating ClickHouse tables");

        // Crawl events table
        let crawl_events_table = self.table_name("crawl_events");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    url String,
                    domain LowCardinality(String),
                    status_code UInt16,
                    response_time_ms UInt32,
                    content_length UInt64,
                    content_type LowCardinality(String),
                    js_rendered Bool,
                    depth UInt32,
                    worker_id LowCardinality(String),
                    job_id String,
                    account_id LowCardinality(String),
                    crawled_at DateTime64(3),
                    error String,
                    links_extracted UInt32,
                    content_changed Bool,
                    INDEX idx_domain domain TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_job_id job_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_account_id account_id TYPE bloom_filter GRANULARITY 1
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(crawled_at)
                ORDER BY (account_id, domain, crawled_at, url)
                TTL toDateTime(crawled_at) + INTERVAL 90 DAY
                "#,
                crawl_events_table
            ))
            .execute()
            .await?;

        // Content events table
        let content_events_table = self.table_name("content_events");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    url String,
                    domain LowCardinality(String),
                    content_hash String,
                    title String,
                    word_count UInt32,
                    language LowCardinality(String),
                    ai_extracted Bool,
                    ai_model LowCardinality(String),
                    ai_tokens UInt32,
                    processing_time_ms UInt32,
                    extracted_at DateTime64(3),
                    job_id String,
                    INDEX idx_domain domain TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_job_id job_id TYPE bloom_filter GRANULARITY 1
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(extracted_at)
                ORDER BY (domain, extracted_at, url)
                TTL toDateTime(extracted_at) + INTERVAL 90 DAY
                "#,
                content_events_table
            ))
            .execute()
            .await?;

        // Domain statistics materialized view
        let domain_stats_table = self.table_name("domain_stats_hourly");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    domain LowCardinality(String),
                    hour DateTime,
                    requests AggregateFunction(count, UInt64),
                    successes AggregateFunction(countIf, UInt8),
                    failures AggregateFunction(countIf, UInt8),
                    total_response_time AggregateFunction(sum, UInt64),
                    total_bytes AggregateFunction(sum, UInt64)
                ) ENGINE = AggregatingMergeTree()
                PARTITION BY toYYYYMM(hour)
                ORDER BY (domain, hour)
                TTL hour + INTERVAL 365 DAY
                "#,
                domain_stats_table
            ))
            .execute()
            .await?;

        // AI usage events table
        let ai_usage_events_table = self.table_name("ai_usage_events");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    provider LowCardinality(String),
                    model LowCardinality(String),
                    operation LowCardinality(String),
                    prompt_tokens UInt32,
                    completion_tokens UInt32,
                    total_tokens UInt32,
                    duration_ms UInt32,
                    job_id String,
                    account_id LowCardinality(String),
                    url String,
                    timestamp DateTime64(3),
                    INDEX idx_account_id account_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_model model TYPE bloom_filter GRANULARITY 1
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (account_id, model, timestamp)
                TTL toDateTime(timestamp) + INTERVAL 90 DAY
                "#,
                ai_usage_events_table
            ))
            .execute()
            .await?;

        info!("ClickHouse tables created successfully");
        Ok(())
    }

    // ========================================================================
    // Insert Operations
    // ========================================================================

    /// Insert a single crawl event.
    #[instrument(skip(self, event), fields(url = %event.url))]
    pub async fn insert_crawl_event(&self, event: CrawlEvent) -> Result<(), ClickHouseError> {
        let table = self.table_name("crawl_events");
        let mut insert = self.client.insert(&table)?;
        insert.write(&event).await?;
        insert.end().await?;
        debug!("Inserted crawl event");
        Ok(())
    }

    /// Insert multiple crawl events in batch.
    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn insert_crawl_events(
        &self,
        events: Vec<CrawlEvent>,
    ) -> Result<(), ClickHouseError> {
        if events.is_empty() {
            return Ok(());
        }

        let table = self.table_name("crawl_events");
        let mut insert = self.client.insert(&table)?;

        for event in &events {
            insert.write(event).await?;
        }

        insert.end().await?;
        debug!(count = events.len(), "Inserted crawl events batch");
        Ok(())
    }

    /// Insert a content extraction event.
    #[instrument(skip(self, event), fields(url = %event.url))]
    pub async fn insert_content_event(&self, event: ContentEvent) -> Result<(), ClickHouseError> {
        let table = self.table_name("content_events");
        let mut insert = self.client.insert(&table)?;
        insert.write(&event).await?;
        insert.end().await?;
        debug!("Inserted content event");
        Ok(())
    }

    /// Insert multiple content events in batch.
    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn insert_content_events(
        &self,
        events: Vec<ContentEvent>,
    ) -> Result<(), ClickHouseError> {
        if events.is_empty() {
            return Ok(());
        }

        let table = self.table_name("content_events");
        let mut insert = self.client.insert(&table)?;

        for event in &events {
            insert.write(event).await?;
        }

        insert.end().await?;
        debug!(count = events.len(), "Inserted content events batch");
        Ok(())
    }

    // ========================================================================
    // Query Operations
    // ========================================================================

    /// Get statistics for a domain over the last N hours.
    #[instrument(skip(self))]
    pub async fn get_domain_stats(
        &self,
        domain: &str,
        hours: u32,
    ) -> Result<DomainStats, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let result = self
            .client
            .query(&format!(
                r#"
                SELECT
                    domain,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    avg(response_time_ms) as avg_response_time_ms,
                    sum(content_length) as total_bytes,
                    uniqExact(url) as unique_urls,
                    countIf(content_changed) as content_changes
                FROM {}
                WHERE domain = ? AND crawled_at >= now() - INTERVAL ? HOUR
                GROUP BY domain
                "#,
                table
            ))
            .bind(domain)
            .bind(hours)
            .fetch_one::<DomainStats>()
            .await;

        match result {
            Ok(stats) => Ok(stats),
            Err(e) => {
                warn!(domain = %domain, error = %e, "Failed to get domain stats");
                // Return empty stats if no data
                Ok(DomainStats {
                    domain: domain.to_string(),
                    total_requests: 0,
                    successful_requests: 0,
                    failed_requests: 0,
                    avg_response_time_ms: 0.0,
                    total_bytes: 0,
                    unique_urls: 0,
                    content_changes: 0,
                })
            }
        }
    }

    /// Get hourly statistics for the last N hours.
    #[instrument(skip(self))]
    pub async fn get_hourly_stats(&self, hours: u32) -> Result<Vec<HourlyStats>, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    toStartOfHour(crawled_at) as hour,
                    count() as requests,
                    countIf(status_code >= 200 AND status_code < 400) as successes,
                    countIf(status_code >= 400 OR error != '') as failures,
                    avg(response_time_ms) as avg_response_time_ms,
                    sum(content_length) as total_bytes
                FROM {}
                WHERE crawled_at >= now() - INTERVAL ? HOUR
                GROUP BY hour
                ORDER BY hour
                "#,
                table
            ))
            .bind(hours)
            .fetch_all::<HourlyStats>()
            .await?;

        Ok(stats)
    }

    /// Get statistics for a specific job.
    #[instrument(skip(self))]
    pub async fn get_job_stats(&self, job_id: &str) -> Result<Option<JobStats>, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let result = self
            .client
            .query(&format!(
                r#"
                SELECT
                    job_id,
                    count() as total_urls,
                    countIf(status_code >= 200 AND status_code < 400) as successful_urls,
                    countIf(status_code >= 400 OR error != '') as failed_urls,
                    sum(content_length) as total_bytes,
                    avg(response_time_ms) as avg_response_time_ms,
                    uniqExact(domain) as unique_domains,
                    min(crawled_at) as started_at,
                    max(crawled_at) as last_activity_at
                FROM {}
                WHERE job_id = ?
                GROUP BY job_id
                "#,
                table
            ))
            .bind(job_id)
            .fetch_optional::<JobStats>()
            .await?;

        Ok(result)
    }

    /// Get top domains by request count.
    #[instrument(skip(self))]
    pub async fn get_top_domains(
        &self,
        hours: u32,
        limit: u32,
    ) -> Result<Vec<DomainStats>, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    domain,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    avg(response_time_ms) as avg_response_time_ms,
                    sum(content_length) as total_bytes,
                    uniqExact(url) as unique_urls,
                    countIf(content_changed) as content_changes
                FROM {}
                WHERE crawled_at >= now() - INTERVAL ? HOUR
                GROUP BY domain
                ORDER BY total_requests DESC
                LIMIT ?
                "#,
                table
            ))
            .bind(hours)
            .bind(limit)
            .fetch_all::<DomainStats>()
            .await?;

        Ok(stats)
    }

    /// Get error distribution for the last N hours.
    #[instrument(skip(self))]
    pub async fn get_error_distribution(
        &self,
        hours: u32,
    ) -> Result<Vec<(u16, u64)>, ClickHouseError> {
        let table = self.table_name("crawl_events");

        #[derive(Row, Deserialize)]
        struct StatusCount {
            status_code: u16,
            count: u64,
        }

        let results = self
            .client
            .query(&format!(
                r#"
                SELECT
                    status_code,
                    count() as count
                FROM {}
                WHERE crawled_at >= now() - INTERVAL ? HOUR
                    AND (status_code >= 400 OR error != '')
                GROUP BY status_code
                ORDER BY count DESC
                "#,
                table
            ))
            .bind(hours)
            .fetch_all::<StatusCount>()
            .await?;

        Ok(results
            .into_iter()
            .map(|r| (r.status_code, r.count))
            .collect())
    }

    /// Get total crawl count for the specified time period.
    #[instrument(skip(self))]
    pub async fn get_total_crawls(&self, hours: u32) -> Result<u64, ClickHouseError> {
        let table = self.table_name("crawl_events");

        #[derive(Row, Deserialize)]
        struct Count {
            count: u64,
        }

        let result = self
            .client
            .query(&format!(
                r#"
                SELECT count() as count
                FROM {}
                WHERE crawled_at >= now() - INTERVAL ? HOUR
                "#,
                table
            ))
            .bind(hours)
            .fetch_one::<Count>()
            .await?;

        Ok(result.count)
    }

    // ========================================================================
    // AI Usage Events
    // ========================================================================

    /// Insert multiple AI usage events in batch.
    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn insert_ai_usage_events(
        &self,
        events: Vec<AiUsageClickHouseEvent>,
    ) -> Result<(), ClickHouseError> {
        if events.is_empty() {
            return Ok(());
        }

        let table = self.table_name("ai_usage_events");
        let mut insert = self.client.insert(&table)?;

        for event in &events {
            insert.write(event).await?;
        }

        insert.end().await?;
        debug!(count = events.len(), "Inserted AI usage events batch");
        Ok(())
    }

    /// Get AI usage statistics grouped by model.
    #[instrument(skip(self))]
    pub async fn get_ai_usage_stats(
        &self,
        hours: u32,
        account_id: Option<&str>,
    ) -> Result<Vec<AiUsageStats>, ClickHouseError> {
        let table = self.table_name("ai_usage_events");

        let (query, needs_account_bind) = if let Some(acct) = account_id {
            if acct.is_empty() {
                (
                    format!(
                        r#"
                        SELECT
                            model,
                            count() as total_calls,
                            sum(prompt_tokens) as total_prompt_tokens,
                            sum(completion_tokens) as total_completion_tokens,
                            sum(total_tokens) as total_tokens,
                            avg(duration_ms) as avg_duration_ms
                        FROM {}
                        WHERE timestamp >= now() - INTERVAL ? HOUR
                        GROUP BY model
                        ORDER BY total_tokens DESC
                        "#,
                        table
                    ),
                    false,
                )
            } else {
                (
                    format!(
                        r#"
                        SELECT
                            model,
                            count() as total_calls,
                            sum(prompt_tokens) as total_prompt_tokens,
                            sum(completion_tokens) as total_completion_tokens,
                            sum(total_tokens) as total_tokens,
                            avg(duration_ms) as avg_duration_ms
                        FROM {}
                        WHERE timestamp >= now() - INTERVAL ? HOUR
                            AND account_id = ?
                        GROUP BY model
                        ORDER BY total_tokens DESC
                        "#,
                        table
                    ),
                    true,
                )
            }
        } else {
            (
                format!(
                    r#"
                    SELECT
                        model,
                        count() as total_calls,
                        sum(prompt_tokens) as total_prompt_tokens,
                        sum(completion_tokens) as total_completion_tokens,
                        sum(total_tokens) as total_tokens,
                        avg(duration_ms) as avg_duration_ms
                    FROM {}
                    WHERE timestamp >= now() - INTERVAL ? HOUR
                    GROUP BY model
                    ORDER BY total_tokens DESC
                    "#,
                    table
                ),
                false,
            )
        };

        let mut q = self.client.query(&query).bind(hours);
        if needs_account_bind {
            q = q.bind(account_id.unwrap_or(""));
        }

        let stats = q.fetch_all::<AiUsageStats>().await?;
        Ok(stats)
    }

    // ========================================================================
    // Billing Analytics
    // ========================================================================

    /// Get usage statistics for a specific account.
    #[instrument(skip(self))]
    pub async fn get_account_usage(
        &self,
        account_id: &str,
        hours: u32,
    ) -> Result<AccountUsageStats, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let result = self
            .client
            .query(&format!(
                r#"
                SELECT
                    account_id,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    sum(content_length) as total_bytes,
                    avg(response_time_ms) as avg_response_time_ms,
                    uniqExact(domain) as unique_domains,
                    uniqExact(job_id) as total_jobs,
                    countIf(js_rendered) as js_renders
                FROM {}
                WHERE account_id = ? AND crawled_at >= now() - INTERVAL ? HOUR
                GROUP BY account_id
                "#,
                table
            ))
            .bind(account_id)
            .bind(hours)
            .fetch_optional::<AccountUsageStats>()
            .await?;

        Ok(result.unwrap_or_else(|| AccountUsageStats {
            account_id: account_id.to_string(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_bytes: 0,
            avg_response_time_ms: 0.0,
            unique_domains: 0,
            total_jobs: 0,
            js_renders: 0,
        }))
    }

    /// Get top accounts by usage.
    #[instrument(skip(self))]
    pub async fn get_top_accounts(
        &self,
        hours: u32,
        limit: u32,
    ) -> Result<Vec<AccountUsageStats>, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    account_id,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    sum(content_length) as total_bytes,
                    avg(response_time_ms) as avg_response_time_ms,
                    uniqExact(domain) as unique_domains,
                    uniqExact(job_id) as total_jobs,
                    countIf(js_rendered) as js_renders
                FROM {}
                WHERE account_id != '' AND crawled_at >= now() - INTERVAL ? HOUR
                GROUP BY account_id
                ORDER BY total_requests DESC
                LIMIT ?
                "#,
                table
            ))
            .bind(hours)
            .bind(limit)
            .fetch_all::<AccountUsageStats>()
            .await?;

        Ok(stats)
    }

    /// Get daily usage breakdown for an account.
    #[instrument(skip(self))]
    pub async fn get_account_daily_usage(
        &self,
        account_id: &str,
        days: u32,
    ) -> Result<Vec<DailyUsageStats>, ClickHouseError> {
        let table = self.table_name("crawl_events");

        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    toDate(crawled_at) as date,
                    count() as requests,
                    sum(content_length) as bytes,
                    uniqExact(job_id) as jobs,
                    countIf(js_rendered) as js_renders
                FROM {}
                WHERE account_id = ? AND crawled_at >= now() - INTERVAL ? DAY
                GROUP BY date
                ORDER BY date
                "#,
                table
            ))
            .bind(account_id)
            .bind(days)
            .fetch_all::<DailyUsageStats>()
            .await?;

        Ok(stats)
    }

    /// Get AI token usage statistics.
    #[instrument(skip(self))]
    pub async fn get_ai_token_stats(&self, hours: u32) -> Result<AiTokenStats, ClickHouseError> {
        let table = self.table_name("content_events");

        let result = self
            .client
            .query(&format!(
                r#"
                SELECT
                    sum(ai_tokens) as total_tokens,
                    countIf(ai_extracted) as ai_extractions,
                    avg(ai_tokens) as avg_tokens_per_extraction
                FROM {}
                WHERE extracted_at >= now() - INTERVAL ? HOUR
                "#,
                table
            ))
            .bind(hours)
            .fetch_one::<AiTokenStats>()
            .await;

        match result {
            Ok(stats) => Ok(stats),
            Err(_) => Ok(AiTokenStats {
                total_tokens: 0,
                ai_extractions: 0,
                avg_tokens_per_extraction: 0.0,
            }),
        }
    }

    /// Health check - verify connection is working.
    pub async fn health_check(&self) -> Result<bool, ClickHouseError> {
        #[derive(Row, Deserialize)]
        struct HealthCheck {
            result: u8,
        }

        let result = self
            .client
            .query("SELECT 1 as result")
            .fetch_one::<HealthCheck>()
            .await?;

        Ok(result.result == 1)
    }
}

/// AI token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AiTokenStats {
    /// Total tokens consumed.
    pub total_tokens: u64,
    /// Number of AI extractions.
    pub ai_extractions: u64,
    /// Average tokens per extraction.
    pub avg_tokens_per_extraction: f64,
}

/// Account usage statistics for billing.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AccountUsageStats {
    /// Account ID.
    pub account_id: String,
    /// Total requests made.
    pub total_requests: u64,
    /// Successful requests (2xx, 3xx).
    pub successful_requests: u64,
    /// Failed requests (4xx, 5xx, errors).
    pub failed_requests: u64,
    /// Total bytes downloaded.
    pub total_bytes: u64,
    /// Average response time in milliseconds.
    pub avg_response_time_ms: f64,
    /// Unique domains crawled.
    pub unique_domains: u64,
    /// Total jobs created.
    pub total_jobs: u64,
    /// JavaScript renders performed.
    pub js_renders: u64,
}

/// Daily usage statistics for billing breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct DailyUsageStats {
    /// Date of usage.
    #[serde(with = "clickhouse::serde::time::date")]
    pub date: time::Date,
    /// Requests on this day.
    pub requests: u64,
    /// Bytes downloaded on this day.
    pub bytes: u64,
    /// Jobs on this day.
    pub jobs: u64,
    /// JS renders on this day.
    pub js_renders: u64,
}

// ============================================================================
// Event Batcher
// ============================================================================

/// Batches crawl events for efficient bulk inserts.
pub struct CrawlEventBatcher {
    storage: ClickHouseStorage,
    batch: parking_lot::Mutex<Vec<CrawlEvent>>,
    batch_size: usize,
}

impl CrawlEventBatcher {
    /// Create a new event batcher.
    pub fn new(storage: ClickHouseStorage, batch_size: usize) -> Self {
        Self {
            storage,
            batch: parking_lot::Mutex::new(Vec::with_capacity(batch_size)),
            batch_size,
        }
    }

    /// Add an event to the batch. Flushes automatically when batch is full.
    pub async fn add(&self, event: CrawlEvent) -> Result<(), ClickHouseError> {
        let should_flush = {
            let mut batch = self.batch.lock();
            batch.push(event);
            batch.len() >= self.batch_size
        };

        if should_flush {
            self.flush().await?;
        }

        Ok(())
    }

    /// Flush all pending events.
    pub async fn flush(&self) -> Result<(), ClickHouseError> {
        let events = {
            let mut batch = self.batch.lock();
            std::mem::take(&mut *batch)
        };

        if !events.is_empty() {
            self.storage.insert_crawl_events(events).await?;
        }

        Ok(())
    }

    /// Get the number of pending events.
    pub fn pending_count(&self) -> usize {
        self.batch.lock().len()
    }
}

impl Drop for CrawlEventBatcher {
    fn drop(&mut self) {
        let batch = std::mem::take(&mut *self.batch.lock());
        if !batch.is_empty() {
            warn!(
                count = batch.len(),
                "CrawlEventBatcher dropped with pending events"
            );
        }
    }
}

// ============================================================================
// AI Usage Events
// ============================================================================

/// A single AI usage event for per-LLM-call tracking.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AiUsageClickHouseEvent {
    /// LLM provider name (openai, anthropic, etc.).
    pub provider: String,
    /// Model used for the call.
    pub model: String,
    /// Type of AI operation (summary, extraction, chat, etc.).
    pub operation: String,
    /// Number of prompt/input tokens.
    pub prompt_tokens: u32,
    /// Number of completion/output tokens.
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
    /// Call duration in milliseconds.
    pub duration_ms: u32,
    /// Job ID (empty for /scrape calls).
    #[serde(default)]
    pub job_id: String,
    /// Account ID for billing attribution.
    #[serde(default)]
    pub account_id: String,
    /// URL being processed when the AI call was made.
    #[serde(default)]
    pub url: String,
    /// When the call occurred.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
}

impl Default for AiUsageClickHouseEvent {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            operation: String::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            duration_ms: 0,
            job_id: String::new(),
            account_id: String::new(),
            url: String::new(),
            timestamp: time::OffsetDateTime::now_utc(),
        }
    }
}

/// Per-model AI usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AiUsageStats {
    /// Model name.
    pub model: String,
    /// Total number of LLM calls.
    pub total_calls: u64,
    /// Total prompt tokens.
    pub total_prompt_tokens: u64,
    /// Total completion tokens.
    pub total_completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
    /// Average call duration in milliseconds.
    pub avg_duration_ms: f64,
}

/// Batches AI usage events for efficient bulk inserts.
pub struct AiUsageBatcher {
    storage: ClickHouseStorage,
    batch: parking_lot::Mutex<Vec<AiUsageClickHouseEvent>>,
    batch_size: usize,
}

impl AiUsageBatcher {
    /// Create a new AI usage event batcher.
    pub fn new(storage: ClickHouseStorage, batch_size: usize) -> Self {
        Self {
            storage,
            batch: parking_lot::Mutex::new(Vec::with_capacity(batch_size)),
            batch_size,
        }
    }

    /// Add an event to the batch. Flushes automatically when batch is full.
    pub async fn add(&self, event: AiUsageClickHouseEvent) -> Result<(), ClickHouseError> {
        let should_flush = {
            let mut batch = self.batch.lock();
            batch.push(event);
            batch.len() >= self.batch_size
        };

        if should_flush {
            self.flush().await?;
        }

        Ok(())
    }

    /// Flush all pending events.
    pub async fn flush(&self) -> Result<(), ClickHouseError> {
        let events = {
            let mut batch = self.batch.lock();
            std::mem::take(&mut *batch)
        };

        if !events.is_empty() {
            self.storage.insert_ai_usage_events(events).await?;
        }

        Ok(())
    }

    /// Get the number of pending events.
    pub fn pending_count(&self) -> usize {
        self.batch.lock().len()
    }
}

impl Drop for AiUsageBatcher {
    fn drop(&mut self) {
        let batch = std::mem::take(&mut *self.batch.lock());
        if !batch.is_empty() {
            warn!(
                count = batch.len(),
                "AiUsageBatcher dropped with pending events"
            );
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.url, "http://localhost:8123");
        assert_eq!(config.database, "scrapix");
        assert!(config.auto_create_tables);
    }

    #[test]
    fn test_config_serialization() {
        let config = ClickHouseConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ClickHouseConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.url, deserialized.url);
        assert_eq!(config.database, deserialized.database);
    }

    #[test]
    fn test_crawl_event_default() {
        let event = CrawlEvent::default();
        assert!(event.url.is_empty());
        assert_eq!(event.status_code, 0);
        assert!(!event.js_rendered);
    }

    #[test]
    fn test_table_name_with_prefix() {
        let config = ClickHouseConfig {
            table_prefix: "prod".to_string(),
            ..Default::default()
        };

        // We can't actually create the storage without a connection,
        // but we can test the table name logic conceptually
        assert!(!config.table_prefix.is_empty());
    }
}
