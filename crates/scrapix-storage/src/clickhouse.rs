//! # ClickHouse Analytics Storage
//!
//! ClickHouse backend for storing request analytics and billing metrics.
//!
//! ## Tables
//!
//! - `request_events` — One row per API call (scrape, map, crawl). The billing atom.
//! - `job_events` — Job lifecycle events (JobStarted, JobCompleted, JobFailed).
//! - `ai_usage_events` — Per-LLM-call tracking for AI cost breakdown.

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, instrument, warn};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Error, Debug)]
pub enum ClickHouseError {
    #[error("Connection failed: {0}")]
    ConnectionError(String),

    #[error("Query failed: {0}")]
    QueryError(#[from] clickhouse::error::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Table not found: {0}")]
    TableNotFound(String),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),
}

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,
    #[serde(default = "default_true")]
    pub auto_create_tables: bool,
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
    pub fn local() -> Self {
        Self::default()
    }

    pub fn production(url: impl Into<String>, database: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            database: database.into(),
            ..Default::default()
        }
    }
}

// ============================================================================
// Data Types — Request Events (billing atom)
// ============================================================================

/// One row per API call. The single source of truth for billing.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct RequestEvent {
    /// Account for billing attribution.
    #[serde(default)]
    pub account_id: String,
    /// Job ID (UUID for crawl, empty for scrape/map).
    #[serde(default)]
    pub job_id: String,
    /// Operation type: "scrape", "map", "crawl".
    pub operation: String,
    /// URL (seed URL for crawl, target URL for scrape/map).
    pub url: String,
    /// Domain extracted from URL.
    pub domain: String,
    /// HTTP status code (0 if N/A).
    pub status_code: u16,
    /// Total duration in milliseconds.
    pub duration_ms: u32,
    /// Total bytes downloaded (bandwidth).
    pub content_length: u64,
    /// Error message if failed.
    #[serde(default)]
    pub error: String,
    /// Whether JS rendering was used.
    #[serde(default)]
    pub js_rendered: bool,
    /// Whether AI summary was requested.
    #[serde(default)]
    pub ai_summary: bool,
    /// Whether AI extraction was requested.
    #[serde(default)]
    pub ai_extraction: bool,
    /// Total AI prompt tokens consumed for this request.
    pub ai_prompt_tokens: u32,
    /// Total AI completion tokens consumed for this request.
    pub ai_completion_tokens: u32,
    /// AI model used (empty if no AI).
    #[serde(default)]
    pub ai_model: String,
    /// For map: number of URLs discovered. 0 otherwise.
    pub urls_found: u32,
    /// For map: number of internal HTTP requests made. 1 for scrape. pages_crawled for crawl.
    pub pages_fetched: u32,
    /// When the request occurred.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
}

impl Default for RequestEvent {
    fn default() -> Self {
        Self {
            account_id: String::new(),
            job_id: String::new(),
            operation: String::new(),
            url: String::new(),
            domain: String::new(),
            status_code: 0,
            duration_ms: 0,
            content_length: 0,
            error: String::new(),
            js_rendered: false,
            ai_summary: false,
            ai_extraction: false,
            ai_prompt_tokens: 0,
            ai_completion_tokens: 0,
            ai_model: String::new(),
            urls_found: 0,
            pages_fetched: 1,
            timestamp: time::OffsetDateTime::now_utc(),
        }
    }
}

// ============================================================================
// Data Types — Job Events (lifecycle only)
// ============================================================================

/// Job lifecycle event (JobStarted, JobCompleted, JobFailed).
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct JobEvent {
    /// Event type: "JobStarted", "JobCompleted", "JobFailed".
    pub event_type: String,
    pub job_id: String,
    #[serde(default)]
    pub account_id: String,
    // -- JobStarted fields --
    #[serde(default)]
    pub index_uid: String,
    pub start_urls: Vec<String>,
    /// "crawl" or "map".
    #[serde(default)]
    pub operation: String,
    /// "http" or "browser".
    #[serde(default)]
    pub crawler_type: String,
    pub max_depth: u32,
    pub max_pages: u64,
    #[serde(default)]
    pub replace_index: bool,
    // -- Feature flags (stored at JobStarted) --
    #[serde(default)]
    pub feature_metadata: bool,
    #[serde(default)]
    pub feature_markdown: bool,
    #[serde(default)]
    pub feature_block_split: bool,
    #[serde(default)]
    pub feature_schema: bool,
    #[serde(default)]
    pub feature_ai_summary: bool,
    #[serde(default)]
    pub feature_ai_extraction: bool,
    // -- JobCompleted fields --
    pub pages_crawled: u64,
    pub documents_indexed: u64,
    pub errors: u64,
    pub bytes_downloaded: u64,
    pub duration_secs: u64,
    // -- JobFailed fields --
    #[serde(default)]
    pub error: String,
    /// When the event occurred.
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
}

impl Default for JobEvent {
    fn default() -> Self {
        Self {
            event_type: String::new(),
            job_id: String::new(),
            account_id: String::new(),
            index_uid: String::new(),
            start_urls: Vec::new(),
            operation: String::new(),
            crawler_type: String::new(),
            max_depth: 0,
            max_pages: 0,
            replace_index: false,
            feature_metadata: false,
            feature_markdown: false,
            feature_block_split: false,
            feature_schema: false,
            feature_ai_summary: false,
            feature_ai_extraction: false,
            pages_crawled: 0,
            documents_indexed: 0,
            errors: 0,
            bytes_downloaded: 0,
            duration_secs: 0,
            error: String::new(),
            timestamp: time::OffsetDateTime::now_utc(),
        }
    }
}

// ============================================================================
// Data Types — AI Usage Events
// ============================================================================

/// Per-LLM-call tracking for AI cost breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AiUsageEvent {
    pub provider: String,
    pub model: String,
    /// Type of AI operation (summary, extraction, etc.).
    pub operation: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub duration_ms: u32,
    #[serde(default)]
    pub job_id: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub url: String,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
}

impl Default for AiUsageEvent {
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

// ============================================================================
// Aggregation Result Types
// ============================================================================

/// Domain-level statistics (from request_events).
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct DomainStats {
    pub domain: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub avg_duration_ms: f64,
    pub total_bytes: u64,
}

/// Hourly statistics (from request_events).
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct HourlyStats {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub hour: time::OffsetDateTime,
    pub requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub avg_duration_ms: f64,
    pub total_bytes: u64,
}

/// Account usage statistics for billing.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AccountUsageStats {
    pub account_id: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_bytes: u64,
    pub avg_duration_ms: f64,
    pub unique_domains: u64,
    pub js_renders: u64,
    pub ai_prompt_tokens: u64,
    pub ai_completion_tokens: u64,
}

/// Daily usage statistics for billing breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct DailyUsageStats {
    #[serde(with = "clickhouse::serde::time::date")]
    pub date: time::Date,
    pub requests: u64,
    pub bytes: u64,
    pub js_renders: u64,
    pub ai_prompt_tokens: u64,
    pub ai_completion_tokens: u64,
}

/// Job-level statistics aggregated from request_events.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct JobStats {
    pub job_id: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_bytes: u64,
    pub avg_duration_ms: f64,
    pub unique_domains: u64,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub started_at: time::OffsetDateTime,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub last_activity_at: time::OffsetDateTime,
}

/// Job event summary row (event type counts).
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct JobEventSummaryRow {
    pub event_type: String,
    pub event_count: u64,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub first_seen: time::OffsetDateTime,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub last_seen: time::OffsetDateTime,
}

/// Per-model AI usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct AiUsageStats {
    pub model: String,
    pub total_calls: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_tokens: u64,
    pub avg_duration_ms: f64,
}

// ============================================================================
// ClickHouse Storage
// ============================================================================

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

        if storage.config.auto_create_tables {
            storage.create_tables().await?;
        }

        Ok(storage)
    }

    pub async fn with_defaults() -> Result<Self, ClickHouseError> {
        Self::new(ClickHouseConfig::default()).await
    }

    fn table_name(&self, name: &str) -> String {
        if self.config.table_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}_{}", self.config.table_prefix, name)
        }
    }

    // ========================================================================
    // Table Creation (drops old tables, creates new schema)
    // ========================================================================

    #[instrument(skip(self))]
    pub async fn create_tables(&self) -> Result<(), ClickHouseError> {
        info!("Creating ClickHouse tables");

        // Drop old tables from previous schema
        for old_table in &[
            "crawl_events",
            "content_events",
            "domain_stats_hourly",
        ] {
            let name = self.table_name(old_table);
            self.client
                .query(&format!("DROP TABLE IF EXISTS {}", name))
                .execute()
                .await?;
        }

        // request_events — one row per API call (billing atom)
        let request_events = self.table_name("request_events");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    account_id     LowCardinality(String),
                    job_id         String,
                    operation      LowCardinality(String),
                    url            String,
                    domain         LowCardinality(String),
                    status_code    UInt16,
                    duration_ms    UInt32,
                    content_length UInt64,
                    error          String,
                    js_rendered    Bool,
                    ai_summary     Bool,
                    ai_extraction  Bool,
                    ai_prompt_tokens     UInt32,
                    ai_completion_tokens UInt32,
                    ai_model       LowCardinality(String),
                    urls_found     UInt32,
                    pages_fetched  UInt32,
                    timestamp      DateTime,
                    INDEX idx_domain domain TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_job_id job_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_account_id account_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_operation operation TYPE set(5) GRANULARITY 1
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (account_id, operation, domain, timestamp)
                TTL timestamp + INTERVAL 90 DAY
                "#,
                request_events
            ))
            .execute()
            .await?;

        // job_events — lifecycle only (JobStarted, JobCompleted, JobFailed)
        let job_events = self.table_name("job_events");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    event_type           LowCardinality(String),
                    job_id               String,
                    account_id           LowCardinality(String),
                    index_uid            String,
                    start_urls           Array(String),
                    operation            LowCardinality(String),
                    crawler_type         LowCardinality(String),
                    max_depth            UInt32,
                    max_pages            UInt64,
                    replace_index        Bool,
                    feature_metadata     Bool,
                    feature_markdown     Bool,
                    feature_block_split  Bool,
                    feature_schema       Bool,
                    feature_ai_summary   Bool,
                    feature_ai_extraction Bool,
                    pages_crawled        UInt64,
                    documents_indexed    UInt64,
                    errors               UInt64,
                    bytes_downloaded     UInt64,
                    duration_secs        UInt64,
                    error                String,
                    timestamp            DateTime,
                    INDEX idx_job_id job_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_account_id account_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_event_type event_type TYPE set(5) GRANULARITY 1
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (job_id, timestamp)
                TTL timestamp + INTERVAL 90 DAY
                "#,
                job_events
            ))
            .execute()
            .await?;

        // ai_usage_events — per-LLM-call tracking
        let ai_usage_events = self.table_name("ai_usage_events");
        self.client
            .query(&format!(
                r#"
                CREATE TABLE IF NOT EXISTS {} (
                    provider             LowCardinality(String),
                    model                LowCardinality(String),
                    operation            LowCardinality(String),
                    prompt_tokens        UInt32,
                    completion_tokens    UInt32,
                    total_tokens         UInt32,
                    duration_ms          UInt32,
                    job_id               String,
                    account_id           LowCardinality(String),
                    url                  String,
                    timestamp            DateTime,
                    INDEX idx_account_id account_id TYPE bloom_filter GRANULARITY 1,
                    INDEX idx_job_id job_id TYPE bloom_filter GRANULARITY 1
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (account_id, model, timestamp)
                TTL timestamp + INTERVAL 90 DAY
                "#,
                ai_usage_events
            ))
            .execute()
            .await?;

        info!("ClickHouse tables created successfully");
        Ok(())
    }

    // ========================================================================
    // Insert Operations
    // ========================================================================

    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn insert_request_events(
        &self,
        events: Vec<RequestEvent>,
    ) -> Result<(), ClickHouseError> {
        if events.is_empty() {
            return Ok(());
        }

        let table = self.table_name("request_events");
        let mut insert = self.client.insert(&table)?;
        for event in &events {
            insert.write(event).await?;
        }
        insert.end().await?;
        debug!(count = events.len(), "Inserted request events batch");
        Ok(())
    }

    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn insert_job_events(&self, events: Vec<JobEvent>) -> Result<(), ClickHouseError> {
        if events.is_empty() {
            return Ok(());
        }

        let table = self.table_name("job_events");
        let mut insert = self.client.insert(&table)?;
        for event in &events {
            insert.write(event).await?;
        }
        insert.end().await?;
        debug!(count = events.len(), "Inserted job events batch");
        Ok(())
    }

    #[instrument(skip(self, events), fields(count = events.len()))]
    pub async fn insert_ai_usage_events(
        &self,
        events: Vec<AiUsageEvent>,
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

    // ========================================================================
    // Query Operations — request_events
    // ========================================================================

    #[instrument(skip(self))]
    pub async fn get_hourly_stats(&self, hours: u32) -> Result<Vec<HourlyStats>, ClickHouseError> {
        let table = self.table_name("request_events");
        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    toStartOfHour(timestamp) as hour,
                    count() as requests,
                    countIf(status_code >= 200 AND status_code < 400) as successes,
                    countIf(status_code >= 400 OR error != '') as failures,
                    avg(duration_ms) as avg_duration_ms,
                    sum(content_length) as total_bytes
                FROM {}
                WHERE timestamp >= now() - INTERVAL ? HOUR
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

    #[instrument(skip(self))]
    pub async fn get_top_domains(
        &self,
        hours: u32,
        limit: u32,
    ) -> Result<Vec<DomainStats>, ClickHouseError> {
        let table = self.table_name("request_events");
        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    domain,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    avg(duration_ms) as avg_duration_ms,
                    sum(content_length) as total_bytes
                FROM {}
                WHERE timestamp >= now() - INTERVAL ? HOUR
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

    #[instrument(skip(self))]
    pub async fn get_domain_stats(
        &self,
        domain: &str,
        hours: u32,
    ) -> Result<DomainStats, ClickHouseError> {
        let table = self.table_name("request_events");
        let result = self
            .client
            .query(&format!(
                r#"
                SELECT
                    domain,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    avg(duration_ms) as avg_duration_ms,
                    sum(content_length) as total_bytes
                FROM {}
                WHERE domain = ? AND timestamp >= now() - INTERVAL ? HOUR
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
            Err(_) => Ok(DomainStats {
                domain: domain.to_string(),
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                avg_duration_ms: 0.0,
                total_bytes: 0,
            }),
        }
    }

    #[instrument(skip(self))]
    pub async fn get_error_distribution(
        &self,
        hours: u32,
    ) -> Result<Vec<(u16, u64)>, ClickHouseError> {
        let table = self.table_name("request_events");

        #[derive(Row, Deserialize)]
        struct StatusCount {
            status_code: u16,
            count: u64,
        }

        let results = self
            .client
            .query(&format!(
                r#"
                SELECT status_code, count() as count
                FROM {}
                WHERE timestamp >= now() - INTERVAL ? HOUR
                    AND (status_code >= 400 OR error != '')
                GROUP BY status_code
                ORDER BY count DESC
                "#,
                table
            ))
            .bind(hours)
            .fetch_all::<StatusCount>()
            .await?;

        Ok(results.into_iter().map(|r| (r.status_code, r.count)).collect())
    }

    // ========================================================================
    // Billing Analytics — request_events
    // ========================================================================

    #[instrument(skip(self))]
    pub async fn get_account_usage(
        &self,
        account_id: &str,
        hours: u32,
    ) -> Result<AccountUsageStats, ClickHouseError> {
        let table = self.table_name("request_events");
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
                    avg(duration_ms) as avg_duration_ms,
                    uniqExact(domain) as unique_domains,
                    countIf(js_rendered) as js_renders,
                    sum(ai_prompt_tokens) as ai_prompt_tokens,
                    sum(ai_completion_tokens) as ai_completion_tokens
                FROM {}
                WHERE account_id = ? AND timestamp >= now() - INTERVAL ? HOUR
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
            avg_duration_ms: 0.0,
            unique_domains: 0,
            js_renders: 0,
            ai_prompt_tokens: 0,
            ai_completion_tokens: 0,
        }))
    }

    #[instrument(skip(self))]
    pub async fn get_top_accounts(
        &self,
        hours: u32,
        limit: u32,
    ) -> Result<Vec<AccountUsageStats>, ClickHouseError> {
        let table = self.table_name("request_events");
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
                    avg(duration_ms) as avg_duration_ms,
                    uniqExact(domain) as unique_domains,
                    countIf(js_rendered) as js_renders,
                    sum(ai_prompt_tokens) as ai_prompt_tokens,
                    sum(ai_completion_tokens) as ai_completion_tokens
                FROM {}
                WHERE account_id != '' AND timestamp >= now() - INTERVAL ? HOUR
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

    #[instrument(skip(self))]
    pub async fn get_account_daily_usage(
        &self,
        account_id: &str,
        days: u32,
    ) -> Result<Vec<DailyUsageStats>, ClickHouseError> {
        let table = self.table_name("request_events");
        let stats = self
            .client
            .query(&format!(
                r#"
                SELECT
                    toDate(timestamp) as date,
                    count() as requests,
                    sum(content_length) as bytes,
                    countIf(js_rendered) as js_renders,
                    sum(ai_prompt_tokens) as ai_prompt_tokens,
                    sum(ai_completion_tokens) as ai_completion_tokens
                FROM {}
                WHERE account_id = ? AND timestamp >= now() - INTERVAL ? DAY
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

    // ========================================================================
    // Query Operations — job_events
    // ========================================================================

    #[instrument(skip(self))]
    pub async fn get_job_events(
        &self,
        job_id: &str,
        limit: u32,
    ) -> Result<Vec<JobEvent>, ClickHouseError> {
        let table = self.table_name("job_events");
        let events = self
            .client
            .query(&format!(
                r#"
                SELECT *
                FROM {}
                WHERE job_id = ?
                ORDER BY timestamp DESC
                LIMIT ?
                "#,
                table
            ))
            .bind(job_id)
            .bind(limit)
            .fetch_all::<JobEvent>()
            .await?;
        Ok(events)
    }

    // ========================================================================
    // Query Operations — job_stats (from request_events)
    // ========================================================================

    #[instrument(skip(self))]
    pub async fn get_job_stats(
        &self,
        job_id: &str,
    ) -> Result<Option<JobStats>, ClickHouseError> {
        let table = self.table_name("request_events");
        let result = self
            .client
            .query(&format!(
                r#"
                SELECT
                    job_id,
                    count() as total_requests,
                    countIf(status_code >= 200 AND status_code < 400) as successful_requests,
                    countIf(status_code >= 400 OR error != '') as failed_requests,
                    sum(content_length) as total_bytes,
                    avg(duration_ms) as avg_duration_ms,
                    uniqExact(domain) as unique_domains,
                    min(timestamp) as started_at,
                    max(timestamp) as last_activity_at
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

    #[instrument(skip(self))]
    pub async fn get_job_event_summary(
        &self,
        job_id: &str,
    ) -> Result<Vec<JobEventSummaryRow>, ClickHouseError> {
        let table = self.table_name("job_events");
        let rows = self
            .client
            .query(&format!(
                r#"
                SELECT
                    event_type,
                    count() as event_count,
                    min(timestamp) as first_seen,
                    max(timestamp) as last_seen
                FROM {}
                WHERE job_id = ?
                GROUP BY event_type
                ORDER BY first_seen
                "#,
                table
            ))
            .bind(job_id)
            .fetch_all::<JobEventSummaryRow>()
            .await?;
        Ok(rows)
    }

    // ========================================================================
    // Query Operations — ai_usage_events
    // ========================================================================

    #[instrument(skip(self))]
    pub async fn get_ai_usage_stats(
        &self,
        hours: u32,
        account_id: Option<&str>,
    ) -> Result<Vec<AiUsageStats>, ClickHouseError> {
        let table = self.table_name("ai_usage_events");

        let has_account = account_id.is_some_and(|a| !a.is_empty());
        let query = if has_account {
            format!(
                r#"
                SELECT model, count() as total_calls,
                    sum(prompt_tokens) as total_prompt_tokens,
                    sum(completion_tokens) as total_completion_tokens,
                    sum(total_tokens) as total_tokens,
                    avg(duration_ms) as avg_duration_ms
                FROM {}
                WHERE timestamp >= now() - INTERVAL ? HOUR AND account_id = ?
                GROUP BY model ORDER BY total_tokens DESC
                "#,
                table
            )
        } else {
            format!(
                r#"
                SELECT model, count() as total_calls,
                    sum(prompt_tokens) as total_prompt_tokens,
                    sum(completion_tokens) as total_completion_tokens,
                    sum(total_tokens) as total_tokens,
                    avg(duration_ms) as avg_duration_ms
                FROM {}
                WHERE timestamp >= now() - INTERVAL ? HOUR
                GROUP BY model ORDER BY total_tokens DESC
                "#,
                table
            )
        };

        let mut q = self.client.query(&query).bind(hours);
        if has_account {
            q = q.bind(account_id.unwrap_or(""));
        }

        let stats = q.fetch_all::<AiUsageStats>().await?;
        Ok(stats)
    }

    // ========================================================================
    // Health Check
    // ========================================================================

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

// ============================================================================
// Generic Event Batcher
// ============================================================================

#[async_trait::async_trait]
pub trait BatchInsert<T: Send>: Send + Sync {
    async fn insert_batch(&self, events: Vec<T>) -> Result<(), ClickHouseError>;
}

#[async_trait::async_trait]
impl BatchInsert<RequestEvent> for ClickHouseStorage {
    async fn insert_batch(&self, events: Vec<RequestEvent>) -> Result<(), ClickHouseError> {
        self.insert_request_events(events).await
    }
}

#[async_trait::async_trait]
impl BatchInsert<JobEvent> for ClickHouseStorage {
    async fn insert_batch(&self, events: Vec<JobEvent>) -> Result<(), ClickHouseError> {
        self.insert_job_events(events).await
    }
}

#[async_trait::async_trait]
impl BatchInsert<AiUsageEvent> for ClickHouseStorage {
    async fn insert_batch(&self, events: Vec<AiUsageEvent>) -> Result<(), ClickHouseError> {
        self.insert_ai_usage_events(events).await
    }
}

pub struct EventBatcher<T: Send> {
    storage: ClickHouseStorage,
    batch: parking_lot::Mutex<Vec<T>>,
    batch_size: usize,
    label: &'static str,
}

impl<T: Send + 'static> EventBatcher<T>
where
    ClickHouseStorage: BatchInsert<T>,
{
    pub fn new(storage: ClickHouseStorage, batch_size: usize, label: &'static str) -> Self {
        Self {
            storage,
            batch: parking_lot::Mutex::new(Vec::with_capacity(batch_size)),
            batch_size,
            label,
        }
    }

    pub async fn add(&self, event: T) -> Result<(), ClickHouseError> {
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

    pub async fn flush(&self) -> Result<(), ClickHouseError> {
        let events = {
            let mut batch = self.batch.lock();
            std::mem::take(&mut *batch)
        };

        if !events.is_empty() {
            self.storage.insert_batch(events).await?;
        }

        Ok(())
    }

    pub fn pending_count(&self) -> usize {
        self.batch.lock().len()
    }
}

impl<T: Send> Drop for EventBatcher<T> {
    fn drop(&mut self) {
        let batch = std::mem::take(&mut *self.batch.lock());
        if !batch.is_empty() {
            warn!(
                count = batch.len(),
                label = self.label,
                "EventBatcher dropped with pending events"
            );
        }
    }
}

/// Batcher type aliases.
pub type RequestEventBatcher = EventBatcher<RequestEvent>;
pub type JobEventBatcher = EventBatcher<JobEvent>;
pub type AiUsageBatcher = EventBatcher<AiUsageEvent>;

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
    fn test_request_event_default() {
        let event = RequestEvent::default();
        assert!(event.url.is_empty());
        assert_eq!(event.status_code, 0);
        assert_eq!(event.pages_fetched, 1);
        assert!(!event.js_rendered);
    }

    #[test]
    fn test_table_name_with_prefix() {
        let config = ClickHouseConfig {
            table_prefix: "prod".to_string(),
            ..Default::default()
        };
        assert!(!config.table_prefix.is_empty());
    }
}
