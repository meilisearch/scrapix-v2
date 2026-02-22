//! # Tinybird-style Analytics API
//!
//! A simple analytics layer on top of ClickHouse that provides
//! pre-defined "pipes" (parameterized SQL queries) as REST endpoints.
//!
//! ## Endpoints
//!
//! All pipes are available at `/analytics/v0/pipes/{pipe_name}.json`
//!
//! ## Available Pipes
//!
//! - `top_domains` - Top domains by request count
//! - `domain_stats` - Statistics for a specific domain
//! - `hourly_stats` - Hourly crawl statistics
//! - `error_distribution` - Error breakdown by status code
//! - `job_stats` - Statistics for a specific job
//! - `recent_errors` - Recent crawl errors
//!
//! ## Example
//!
//! ```bash
//! # Get top 10 domains from last 24 hours
//! curl "http://localhost:8080/analytics/v0/pipes/top_domains.json?hours=24&limit=10"
//!
//! # Get stats for a specific domain
//! curl "http://localhost:8080/analytics/v0/pipes/domain_stats.json?domain=example.com&hours=24"
//! ```

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use scrapix_storage::clickhouse::{ClickHouseConfig, ClickHouseStorage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

// ============================================================================
// Configuration
// ============================================================================

/// Analytics configuration
#[derive(Debug, Clone)]
pub struct AnalyticsConfig {
    pub clickhouse_url: String,
    pub clickhouse_database: String,
    pub clickhouse_user: Option<String>,
    pub clickhouse_password: Option<String>,
}

impl AnalyticsConfig {
    /// Create from environment variables
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("CLICKHOUSE_URL").ok()?;
        Some(Self {
            clickhouse_url: url,
            clickhouse_database: std::env::var("CLICKHOUSE_DATABASE")
                .unwrap_or_else(|_| "scrapix".to_string()),
            clickhouse_user: std::env::var("CLICKHOUSE_USER").ok(),
            clickhouse_password: std::env::var("CLICKHOUSE_PASSWORD").ok(),
        })
    }
}

// ============================================================================
// Analytics State
// ============================================================================

/// Shared state for analytics endpoints
pub struct AnalyticsState {
    /// ClickHouse storage client (public for sharing with event persistence)
    pub storage: ClickHouseStorage,
}

impl AnalyticsState {
    /// Create analytics state with provided storage
    pub fn with_storage(storage: ClickHouseStorage) -> Self {
        Self { storage }
    }

    /// Create analytics state from config
    #[allow(dead_code)]
    pub async fn new(config: AnalyticsConfig) -> Result<Self, String> {
        let ch_config = ClickHouseConfig {
            url: config.clickhouse_url,
            database: config.clickhouse_database,
            username: config.clickhouse_user,
            password: config.clickhouse_password,
            auto_create_tables: true,
            ..Default::default()
        };

        let storage = ClickHouseStorage::new(ch_config)
            .await
            .map_err(|e| format!("Failed to connect to ClickHouse: {}", e))?;

        info!("Analytics backend connected to ClickHouse");
        Ok(Self { storage })
    }
}

// ============================================================================
// Response Types (Tinybird-style)
// ============================================================================

/// Standard analytics response wrapper
#[derive(Debug, Serialize)]
pub struct AnalyticsResponse<T> {
    pub meta: Vec<ColumnMeta>,
    pub data: Vec<T>,
    pub rows: usize,
    pub statistics: QueryStats,
}

/// Column metadata
#[derive(Debug, Serialize)]
pub struct ColumnMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub col_type: String,
}

/// Query statistics
#[derive(Debug, Serialize)]
pub struct QueryStats {
    pub elapsed: f64,
    pub rows_read: usize,
    pub bytes_read: usize,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct AnalyticsError {
    pub error: String,
    pub code: String,
}

// ============================================================================
// Pipe: top_domains
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TopDomainsParams {
    #[serde(default = "default_hours")]
    hours: u32,
    #[serde(default = "default_limit")]
    limit: u32,
}

#[derive(Debug, Serialize)]
pub struct TopDomainRow {
    pub domain: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub total_bytes: u64,
    pub unique_urls: u64,
}

async fn pipe_top_domains(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<TopDomainsParams>,
) -> Result<Json<AnalyticsResponse<TopDomainRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    let stats = state
        .storage
        .get_top_domains(params.hours, params.limit)
        .await
        .map_err(|e| {
            error!("top_domains query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let data: Vec<TopDomainRow> = stats
        .into_iter()
        .map(|s| {
            let success_rate = if s.total_requests > 0 {
                s.successful_requests as f64 / s.total_requests as f64 * 100.0
            } else {
                0.0
            };
            TopDomainRow {
                domain: s.domain,
                total_requests: s.total_requests,
                successful_requests: s.successful_requests,
                failed_requests: s.failed_requests,
                success_rate,
                avg_response_time_ms: s.avg_response_time_ms,
                total_bytes: s.total_bytes,
                unique_urls: s.unique_urls,
            }
        })
        .collect();

    let rows = data.len();
    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "domain".into(),
                col_type: "String".into(),
            },
            ColumnMeta {
                name: "total_requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "successful_requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "failed_requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "success_rate".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "avg_response_time_ms".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "total_bytes".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "unique_urls".into(),
                col_type: "UInt64".into(),
            },
        ],
        data,
        rows,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: rows,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipe: domain_stats
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct DomainStatsParams {
    domain: String,
    #[serde(default = "default_hours")]
    hours: u32,
}

async fn pipe_domain_stats(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<DomainStatsParams>,
) -> Result<Json<AnalyticsResponse<TopDomainRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    let stats = state
        .storage
        .get_domain_stats(&params.domain, params.hours)
        .await
        .map_err(|e| {
            error!("domain_stats query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let success_rate = if stats.total_requests > 0 {
        stats.successful_requests as f64 / stats.total_requests as f64 * 100.0
    } else {
        0.0
    };

    let data = vec![TopDomainRow {
        domain: stats.domain,
        total_requests: stats.total_requests,
        successful_requests: stats.successful_requests,
        failed_requests: stats.failed_requests,
        success_rate,
        avg_response_time_ms: stats.avg_response_time_ms,
        total_bytes: stats.total_bytes,
        unique_urls: stats.unique_urls,
    }];

    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "domain".into(),
                col_type: "String".into(),
            },
            ColumnMeta {
                name: "total_requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "successful_requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "failed_requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "success_rate".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "avg_response_time_ms".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "total_bytes".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "unique_urls".into(),
                col_type: "UInt64".into(),
            },
        ],
        data,
        rows: 1,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: 1,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipe: hourly_stats
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct HourlyStatsParams {
    #[serde(default = "default_hours")]
    hours: u32,
}

#[derive(Debug, Serialize)]
pub struct HourlyStatsRow {
    pub hour: String,
    pub requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub total_bytes: u64,
}

async fn pipe_hourly_stats(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<HourlyStatsParams>,
) -> Result<Json<AnalyticsResponse<HourlyStatsRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    let stats = state
        .storage
        .get_hourly_stats(params.hours)
        .await
        .map_err(|e| {
            error!("hourly_stats query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let data: Vec<HourlyStatsRow> = stats
        .into_iter()
        .map(|s| {
            let success_rate = if s.requests > 0 {
                s.successes as f64 / s.requests as f64 * 100.0
            } else {
                0.0
            };
            HourlyStatsRow {
                hour: s.hour.to_string(),
                requests: s.requests,
                successes: s.successes,
                failures: s.failures,
                success_rate,
                avg_response_time_ms: s.avg_response_time_ms,
                total_bytes: s.total_bytes,
            }
        })
        .collect();

    let rows = data.len();
    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "hour".into(),
                col_type: "DateTime".into(),
            },
            ColumnMeta {
                name: "requests".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "successes".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "failures".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "success_rate".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "avg_response_time_ms".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "total_bytes".into(),
                col_type: "UInt64".into(),
            },
        ],
        data,
        rows,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: rows,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipe: error_distribution
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ErrorDistributionRow {
    pub status_code: u16,
    pub count: u64,
    pub percentage: f64,
}

async fn pipe_error_distribution(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<HourlyStatsParams>,
) -> Result<Json<AnalyticsResponse<ErrorDistributionRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    let distribution = state
        .storage
        .get_error_distribution(params.hours)
        .await
        .map_err(|e| {
            error!("error_distribution query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let total: u64 = distribution.iter().map(|(_, c)| c).sum();
    let data: Vec<ErrorDistributionRow> = distribution
        .into_iter()
        .map(|(status_code, count)| {
            let percentage = if total > 0 {
                count as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            ErrorDistributionRow {
                status_code,
                count,
                percentage,
            }
        })
        .collect();

    let rows = data.len();
    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "status_code".into(),
                col_type: "UInt16".into(),
            },
            ColumnMeta {
                name: "count".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "percentage".into(),
                col_type: "Float64".into(),
            },
        ],
        data,
        rows,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: rows,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipe: job_stats
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct JobStatsParams {
    job_id: String,
}

#[derive(Debug, Serialize)]
pub struct JobStatsRow {
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

async fn pipe_job_stats(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<JobStatsParams>,
) -> Result<Json<AnalyticsResponse<JobStatsRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    let stats = state
        .storage
        .get_job_stats(&params.job_id)
        .await
        .map_err(|e| {
            error!("job_stats query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let data = match stats {
        Some(s) => {
            let success_rate = if s.total_urls > 0 {
                s.successful_urls as f64 / s.total_urls as f64 * 100.0
            } else {
                0.0
            };
            let duration = (s.last_activity_at - s.started_at).whole_seconds();
            vec![JobStatsRow {
                job_id: s.job_id,
                total_urls: s.total_urls,
                successful_urls: s.successful_urls,
                failed_urls: s.failed_urls,
                success_rate,
                total_bytes: s.total_bytes,
                avg_response_time_ms: s.avg_response_time_ms,
                unique_domains: s.unique_domains,
                started_at: s.started_at.to_string(),
                last_activity_at: s.last_activity_at.to_string(),
                duration_seconds: duration,
            }]
        }
        None => vec![],
    };

    let rows = data.len();
    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "job_id".into(),
                col_type: "String".into(),
            },
            ColumnMeta {
                name: "total_urls".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "successful_urls".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "failed_urls".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "success_rate".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "total_bytes".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "avg_response_time_ms".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "unique_domains".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "started_at".into(),
                col_type: "DateTime".into(),
            },
            ColumnMeta {
                name: "last_activity_at".into(),
                col_type: "DateTime".into(),
            },
            ColumnMeta {
                name: "duration_seconds".into(),
                col_type: "Int64".into(),
            },
        ],
        data,
        rows,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: rows,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipe: kpis (Key Performance Indicators)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct KpisRow {
    pub total_crawls: u64,
    pub total_bytes: u64,
    pub unique_domains: u64,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub errors_count: u64,
}

async fn pipe_kpis(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<HourlyStatsParams>,
) -> Result<Json<AnalyticsResponse<KpisRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    // Get top domains to aggregate stats
    let domains = state
        .storage
        .get_top_domains(params.hours, 10000)
        .await
        .map_err(|e| {
            error!("kpis query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let mut total_crawls = 0u64;
    let mut total_successes = 0u64;
    let mut total_bytes = 0u64;
    let mut total_response_time = 0f64;
    let mut errors_count = 0u64;

    for d in &domains {
        total_crawls += d.total_requests;
        total_successes += d.successful_requests;
        total_bytes += d.total_bytes;
        total_response_time += d.avg_response_time_ms * d.total_requests as f64;
        errors_count += d.failed_requests;
    }

    let success_rate = if total_crawls > 0 {
        total_successes as f64 / total_crawls as f64 * 100.0
    } else {
        0.0
    };

    let avg_response_time_ms = if total_crawls > 0 {
        total_response_time / total_crawls as f64
    } else {
        0.0
    };

    let data = vec![KpisRow {
        total_crawls,
        total_bytes,
        unique_domains: domains.len() as u64,
        success_rate,
        avg_response_time_ms,
        errors_count,
    }];

    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "total_crawls".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "total_bytes".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "unique_domains".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "success_rate".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "avg_response_time_ms".into(),
                col_type: "Float64".into(),
            },
            ColumnMeta {
                name: "errors_count".into(),
                col_type: "UInt64".into(),
            },
        ],
        data,
        rows: 1,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: 1,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipe: ai_usage
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct AiUsageParams {
    #[serde(default = "default_hours")]
    hours: u32,
    #[serde(default)]
    account_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AiUsageRow {
    pub model: String,
    pub total_calls: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_tokens: u64,
    pub avg_duration_ms: f64,
}

async fn pipe_ai_usage(
    State(state): State<Arc<AnalyticsState>>,
    Query(params): Query<AiUsageParams>,
) -> Result<Json<AnalyticsResponse<AiUsageRow>>, (StatusCode, Json<AnalyticsError>)> {
    let start = Instant::now();

    let stats = state
        .storage
        .get_ai_usage_stats(params.hours, params.account_id.as_deref())
        .await
        .map_err(|e| {
            error!("ai_usage query failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AnalyticsError {
                    error: e.to_string(),
                    code: "QUERY_ERROR".to_string(),
                }),
            )
        })?;

    let data: Vec<AiUsageRow> = stats
        .into_iter()
        .map(|s| AiUsageRow {
            model: s.model,
            total_calls: s.total_calls,
            total_prompt_tokens: s.total_prompt_tokens,
            total_completion_tokens: s.total_completion_tokens,
            total_tokens: s.total_tokens,
            avg_duration_ms: s.avg_duration_ms,
        })
        .collect();

    let rows = data.len();
    Ok(Json(AnalyticsResponse {
        meta: vec![
            ColumnMeta {
                name: "model".into(),
                col_type: "String".into(),
            },
            ColumnMeta {
                name: "total_calls".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "total_prompt_tokens".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "total_completion_tokens".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "total_tokens".into(),
                col_type: "UInt64".into(),
            },
            ColumnMeta {
                name: "avg_duration_ms".into(),
                col_type: "Float64".into(),
            },
        ],
        data,
        rows,
        statistics: QueryStats {
            elapsed: start.elapsed().as_secs_f64(),
            rows_read: rows,
            bytes_read: 0,
        },
    }))
}

// ============================================================================
// Pipes List
// ============================================================================

#[derive(Debug, Serialize)]
pub struct PipeInfo {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ParamInfo>,
    pub endpoint: String,
}

#[derive(Debug, Serialize)]
pub struct ParamInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub required: bool,
    pub default: Option<String>,
}

async fn list_pipes() -> Json<Vec<PipeInfo>> {
    Json(vec![
        PipeInfo {
            name: "top_domains".into(),
            description: "Top domains by request count".into(),
            parameters: vec![
                ParamInfo {
                    name: "hours".into(),
                    param_type: "integer".into(),
                    required: false,
                    default: Some("24".into()),
                },
                ParamInfo {
                    name: "limit".into(),
                    param_type: "integer".into(),
                    required: false,
                    default: Some("20".into()),
                },
            ],
            endpoint: "/analytics/v0/pipes/top_domains.json".into(),
        },
        PipeInfo {
            name: "domain_stats".into(),
            description: "Statistics for a specific domain".into(),
            parameters: vec![
                ParamInfo {
                    name: "domain".into(),
                    param_type: "string".into(),
                    required: true,
                    default: None,
                },
                ParamInfo {
                    name: "hours".into(),
                    param_type: "integer".into(),
                    required: false,
                    default: Some("24".into()),
                },
            ],
            endpoint: "/analytics/v0/pipes/domain_stats.json".into(),
        },
        PipeInfo {
            name: "hourly_stats".into(),
            description: "Hourly crawl statistics".into(),
            parameters: vec![ParamInfo {
                name: "hours".into(),
                param_type: "integer".into(),
                required: false,
                default: Some("24".into()),
            }],
            endpoint: "/analytics/v0/pipes/hourly_stats.json".into(),
        },
        PipeInfo {
            name: "error_distribution".into(),
            description: "Error breakdown by status code".into(),
            parameters: vec![ParamInfo {
                name: "hours".into(),
                param_type: "integer".into(),
                required: false,
                default: Some("24".into()),
            }],
            endpoint: "/analytics/v0/pipes/error_distribution.json".into(),
        },
        PipeInfo {
            name: "job_stats".into(),
            description: "Statistics for a specific job".into(),
            parameters: vec![ParamInfo {
                name: "job_id".into(),
                param_type: "string".into(),
                required: true,
                default: None,
            }],
            endpoint: "/analytics/v0/pipes/job_stats.json".into(),
        },
        PipeInfo {
            name: "kpis".into(),
            description: "Key performance indicators summary".into(),
            parameters: vec![ParamInfo {
                name: "hours".into(),
                param_type: "integer".into(),
                required: false,
                default: Some("24".into()),
            }],
            endpoint: "/analytics/v0/pipes/kpis.json".into(),
        },
        PipeInfo {
            name: "ai_usage".into(),
            description: "AI/LLM token usage per model".into(),
            parameters: vec![
                ParamInfo {
                    name: "hours".into(),
                    param_type: "integer".into(),
                    required: false,
                    default: Some("24".into()),
                },
                ParamInfo {
                    name: "account_id".into(),
                    param_type: "string".into(),
                    required: false,
                    default: None,
                },
            ],
            endpoint: "/analytics/v0/pipes/ai_usage.json".into(),
        },
    ])
}

// ============================================================================
// Router
// ============================================================================

fn default_hours() -> u32 {
    24
}

fn default_limit() -> u32 {
    20
}

/// Create the analytics router
pub fn create_analytics_router(state: Arc<AnalyticsState>) -> Router {
    Router::new()
        .route("/pipes", get(list_pipes))
        .route("/pipes/top_domains.json", get(pipe_top_domains))
        .route("/pipes/domain_stats.json", get(pipe_domain_stats))
        .route("/pipes/hourly_stats.json", get(pipe_hourly_stats))
        .route(
            "/pipes/error_distribution.json",
            get(pipe_error_distribution),
        )
        .route("/pipes/job_stats.json", get(pipe_job_stats))
        .route("/pipes/kpis.json", get(pipe_kpis))
        .route("/pipes/ai_usage.json", get(pipe_ai_usage))
        .with_state(state)
}

/// Try to initialize analytics (returns None if ClickHouse not configured)
#[allow(dead_code)]
pub async fn try_init_analytics() -> Option<Arc<AnalyticsState>> {
    let config = AnalyticsConfig::from_env()?;

    match AnalyticsState::new(config).await {
        Ok(state) => {
            info!("Analytics API initialized with ClickHouse backend");
            Some(Arc::new(state))
        }
        Err(e) => {
            warn!(
                "Failed to initialize analytics: {}. Analytics endpoints will be disabled.",
                e
            );
            None
        }
    }
}
