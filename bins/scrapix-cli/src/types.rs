use serde::{Deserialize, Serialize};
use tabled::Tabled;

// ============================================================================
// API Error
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub details: Option<serde_json::Value>,
}

// ============================================================================
// Generic message response
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

// ============================================================================
// Health
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub kafka_connected: bool,
}

// ============================================================================
// Crawl / Jobs
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateCrawlResponse {
    pub job_id: String,
    pub status: String,
    pub index_uid: String,
    pub start_urls_count: usize,
    #[allow(dead_code)]
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub status: String,
    pub index_uid: String,
    pub pages_crawled: u64,
    pub pages_indexed: u64,
    pub documents_sent: u64,
    pub errors: u64,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub error_message: Option<String>,
    pub crawl_rate: f64,
    pub eta_seconds: Option<u64>,
}

#[derive(Tabled)]
pub struct JobRow {
    #[tabled(rename = "Job ID")]
    pub job_id: String,
    #[tabled(rename = "Status")]
    pub status: String,
    #[tabled(rename = "Index")]
    pub index_uid: String,
    #[tabled(rename = "Crawled")]
    pub pages_crawled: u64,
    #[tabled(rename = "Indexed")]
    pub pages_indexed: u64,
    #[tabled(rename = "Errors")]
    pub errors: u64,
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
// Scrape
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ScrapeRequest {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_render: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_options: Option<ScrapeAiOptions>,
}

#[derive(Debug, Serialize)]
pub struct ScrapeAiOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_prompt: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScrapeResponse {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub raw_html: Option<String>,
    #[serde(default)]
    pub links: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub ai_summary: Option<String>,
    #[serde(default)]
    pub ai_extraction: Option<serde_json::Value>,
    #[serde(default)]
    pub status_code: Option<u16>,
}

// ============================================================================
// Map
// ============================================================================

#[derive(Debug, Serialize)]
pub struct MapRequest {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_sitemap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_metadata: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MapResponse {
    pub url: String,
    pub links: Vec<MapLink>,
    pub total: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MapLink {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

// ============================================================================
// Search
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SearchRequest {
    pub url: String,
    pub q: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<serde_json::Value>,
    pub query: String,
    #[serde(default)]
    pub processing_time_ms: Option<u64>,
    #[serde(default)]
    pub estimated_total_hits: Option<u64>,
}

// ============================================================================
// Configs
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CrawlConfigRecord {
    pub id: String,
    pub account_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub config: serde_json::Value,
    #[serde(default)]
    pub cron_expression: Option<String>,
    #[serde(default)]
    pub cron_enabled: Option<bool>,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub next_run_at: Option<String>,
    #[serde(default)]
    pub last_job_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Tabled)]
pub struct ConfigRow {
    #[tabled(rename = "ID")]
    pub id: String,
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Cron")]
    pub cron: String,
    #[tabled(rename = "Enabled")]
    pub enabled: String,
    #[tabled(rename = "Last Run")]
    pub last_run: String,
}

// ============================================================================
// Engines
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct EngineRecord {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateEngineRequest {
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct UpdateEngineRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Tabled)]
pub struct EngineRow {
    #[tabled(rename = "ID")]
    pub id: String,
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "URL")]
    pub url: String,
    #[tabled(rename = "Default")]
    pub is_default: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EngineIndex {
    pub uid: String,
    #[serde(default)]
    pub primary_key: Option<String>,
    #[serde(default)]
    pub number_of_documents: Option<u64>,
}

// ============================================================================
// API Keys
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub prefix: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub revoked: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Tabled)]
pub struct ApiKeyRow {
    #[tabled(rename = "ID")]
    pub id: String,
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Prefix")]
    pub prefix: String,
    #[tabled(rename = "Revoked")]
    pub revoked: String,
}

// ============================================================================
// Billing
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct BillingResponse {
    pub credits_balance: i64,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub auto_topup_enabled: Option<bool>,
    #[serde(default)]
    pub auto_topup_amount: Option<i64>,
    #[serde(default)]
    pub monthly_spend_limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TransactionRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub amount: i64,
    pub balance_after: i64,
    #[serde(default)]
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Tabled)]
pub struct TransactionRow {
    #[tabled(rename = "Date")]
    pub date: String,
    #[tabled(rename = "Type")]
    pub tx_type: String,
    #[tabled(rename = "Amount")]
    pub amount: String,
    #[tabled(rename = "Balance")]
    pub balance: String,
    #[tabled(rename = "Description")]
    pub description: String,
}

// ============================================================================
// Team
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct TeamMember {
    pub user_id: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub role: String,
    #[serde(default)]
    pub joined_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InviteMemberRequest {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

#[derive(Tabled)]
pub struct TeamMemberRow {
    #[tabled(rename = "User ID")]
    pub user_id: String,
    #[tabled(rename = "Email")]
    pub email: String,
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Role")]
    pub role: String,
}

// ============================================================================
// Auth
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct WhoamiResponse {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub account_name: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub credits_balance: Option<i64>,
}

// ============================================================================
// Diagnostics
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct SystemStatsResponse {
    pub meilisearch: Option<MeilisearchStats>,
    pub jobs: JobSummary,
    pub diagnostics: DiagnosticsStats,
    pub collected_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MeilisearchStats {
    pub available: bool,
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JobSummary {
    pub total: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiagnosticsStats {
    pub recent_errors_count: usize,
    pub tracked_domains: usize,
    pub total_requests: u64,
    pub total_successes: u64,
    pub total_failures: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ErrorsResponse {
    pub errors: Vec<ErrorRecord>,
    pub total_count: usize,
    pub by_status: std::collections::HashMap<String, u64>,
    pub by_domain: Vec<(String, u64)>,
    pub source: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ErrorRecord {
    pub url: String,
    pub domain: String,
    pub error: String,
    pub status_code: Option<u16>,
    pub job_id: String,
    pub timestamp: String,
    pub retry_count: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DomainsResponse {
    pub domains: Vec<DomainInfo>,
    pub total_domains: usize,
    pub source: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DomainInfo {
    pub domain: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub avg_response_time_ms: Option<f64>,
}

#[derive(Tabled)]
pub struct DomainRow {
    #[tabled(rename = "Domain")]
    pub domain: String,
    #[tabled(rename = "Requests")]
    pub requests: u64,
    #[tabled(rename = "Success")]
    pub success: String,
    #[tabled(rename = "Failed")]
    pub failed: u64,
    #[tabled(rename = "Avg Time")]
    pub avg_time: String,
}

// ============================================================================
// Analytics (Tinybird-style)
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct AnalyticsResponse<T> {
    pub meta: Vec<ColumnMeta>,
    pub data: Vec<T>,
    pub rows: usize,
    pub statistics: QueryStats,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ColumnMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub col_type: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct QueryStats {
    pub elapsed: f64,
    pub rows_read: usize,
    pub bytes_read: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PipeInfo {
    pub name: String,
    pub description: String,
    pub endpoint: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct KpisData {
    pub total_crawls: u64,
    pub total_bytes: u64,
    pub unique_domains: u64,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub errors_count: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TopDomainData {
    pub domain: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub total_bytes: u64,
    pub unique_urls: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HourlyStatsData {
    pub hour: String,
    pub requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub total_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ErrorDistData {
    pub status_code: u16,
    pub count: u64,
    pub percentage: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JobStatsData {
    pub job_id: String,
    pub total_urls: u64,
    pub successful_urls: u64,
    pub failed_urls: u64,
    pub success_rate: f64,
    pub total_bytes: u64,
    pub avg_response_time_ms: f64,
    pub unique_domains: u64,
    pub started_at: String,
    pub last_activity_at: String,
    pub duration_seconds: i64,
}

#[derive(Tabled)]
pub struct TopDomainAnalyticsRow {
    #[tabled(rename = "Domain")]
    pub domain: String,
    #[tabled(rename = "Requests")]
    pub requests: u64,
    #[tabled(rename = "Success")]
    pub success: String,
    #[tabled(rename = "Failed")]
    pub failed: u64,
    #[tabled(rename = "Avg Time")]
    pub avg_time: String,
    #[tabled(rename = "Bytes")]
    pub bytes: String,
}

#[derive(Tabled)]
pub struct HourlyRow {
    #[tabled(rename = "Hour")]
    pub hour: String,
    #[tabled(rename = "Requests")]
    pub requests: u64,
    #[tabled(rename = "Success")]
    pub success: String,
    #[tabled(rename = "Failed")]
    pub failed: u64,
    #[tabled(rename = "Avg Time")]
    pub avg_time: String,
}

#[derive(Tabled)]
pub struct ErrorDistRow {
    #[tabled(rename = "Status")]
    pub status: u16,
    #[tabled(rename = "Count")]
    pub count: u64,
    #[tabled(rename = "Percentage")]
    pub percentage: String,
}

// ============================================================================
// Local crawl
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct LocalCrawlDocument {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content: String,
    pub markdown: Option<String>,
    pub crawled_at: String,
    pub status_code: u16,
    pub depth: u32,
}

#[derive(Debug, Serialize)]
pub struct LocalCrawlResult {
    pub index_uid: String,
    pub pages_crawled: u64,
    pub pages_failed: u64,
    pub duration_seconds: f64,
    pub documents: Vec<LocalCrawlDocument>,
}

// ============================================================================
// Paginated response wrapper
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}
