//! PostgreSQL persistence for crawl job state.
//!
//! Write-through cache: the in-memory HashMap remains the primary read path,
//! while Postgres provides durability across restarts.

use scrapix_core::{JobState, JobStatus};
use sqlx::PgPool;
use tracing::{debug, warn};

// ============================================================================
// Status conversion helpers
// ============================================================================

fn status_to_str(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
        JobStatus::Paused => "paused",
    }
}

fn str_to_status(s: &str) -> JobStatus {
    match s {
        "pending" => JobStatus::Pending,
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        "paused" => JobStatus::Paused,
        _ => JobStatus::Pending,
    }
}

// ============================================================================
// Row → JobState conversion
// ============================================================================

fn row_to_job_state(row: &sqlx::postgres::PgRow) -> JobState {
    use sqlx::Row;
    let status_str: String = row.get("status");
    let start_urls: serde_json::Value = row.get("start_urls");
    let account_id: Option<uuid::Uuid> = row.get("account_id");

    JobState {
        job_id: row.get("job_id"),
        status: str_to_status(&status_str),
        index_uid: row.get("index_uid"),
        account_id: account_id.map(|u| u.to_string()),
        pages_crawled: row.get::<i64, _>("pages_crawled") as u64,
        pages_indexed: row.get::<i64, _>("pages_indexed") as u64,
        documents_sent: row.get::<i64, _>("documents_sent") as u64,
        errors: row.get::<i64, _>("errors") as u64,
        bytes_downloaded: row.get::<i64, _>("bytes_downloaded") as u64,
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        error_message: row.get("error_message"),
        crawl_rate: row.get("crawl_rate"),
        eta_seconds: row.get::<Option<i64>, _>("eta_seconds").map(|v| v as u64),
        start_urls: serde_json::from_value(start_urls).unwrap_or_default(),
        max_pages: row.get::<Option<i64>, _>("max_pages").map(|v| v as u64),
        config: row.get("config"),
        swap_temp_index: row.get("swap_temp_index"),
        swap_meilisearch_url: row.get("swap_meilisearch_url"),
        swap_meilisearch_api_key: row.get("swap_meilisearch_api_key"),
    }
}

// ============================================================================
// Insert / Update
// ============================================================================

/// Insert a new job row. Uses ON CONFLICT DO NOTHING for idempotency.
pub async fn insert_job(pool: &PgPool, job: &JobState) {
    let account_id: Option<uuid::Uuid> = job.account_id.as_deref().and_then(|s| s.parse().ok());

    let start_urls = serde_json::to_value(&job.start_urls).unwrap_or_default();

    let result = sqlx::query(
        "INSERT INTO jobs (
            job_id, status, index_uid, account_id,
            pages_crawled, pages_indexed, documents_sent, errors, bytes_downloaded,
            started_at, completed_at, crawl_rate, eta_seconds,
            error_message, start_urls, max_pages, config,
            swap_temp_index, swap_meilisearch_url, swap_meilisearch_api_key
        ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7, $8, $9,
            $10, $11, $12, $13,
            $14, $15, $16, $17,
            $18, $19, $20
        ) ON CONFLICT (job_id) DO NOTHING",
    )
    .bind(&job.job_id)
    .bind(status_to_str(&job.status))
    .bind(&job.index_uid)
    .bind(account_id)
    .bind(job.pages_crawled as i64)
    .bind(job.pages_indexed as i64)
    .bind(job.documents_sent as i64)
    .bind(job.errors as i64)
    .bind(job.bytes_downloaded as i64)
    .bind(job.started_at)
    .bind(job.completed_at)
    .bind(job.crawl_rate)
    .bind(job.eta_seconds.map(|v| v as i64))
    .bind(&job.error_message)
    .bind(&start_urls)
    .bind(job.max_pages.map(|v| v as i64))
    .bind(&job.config)
    .bind(&job.swap_temp_index)
    .bind(&job.swap_meilisearch_url)
    .bind(&None::<String>) // Never persist Meilisearch API key to database
    .execute(pool)
    .await;

    if let Err(e) = result {
        warn!(job_id = %job.job_id, error = %e, "Failed to insert job into Postgres");
    }
}

/// Full update of a single job's mutable fields (lifecycle events: complete, fail, cancel).
pub async fn update_job_full(pool: &PgPool, job: &JobState) {
    let result = sqlx::query(
        "UPDATE jobs SET
            status = $2,
            pages_crawled = $3, pages_indexed = $4, documents_sent = $5,
            errors = $6, bytes_downloaded = $7,
            started_at = $8, completed_at = $9,
            crawl_rate = $10, eta_seconds = $11,
            error_message = $12
        WHERE job_id = $1",
    )
    .bind(&job.job_id)
    .bind(status_to_str(&job.status))
    .bind(job.pages_crawled as i64)
    .bind(job.pages_indexed as i64)
    .bind(job.documents_sent as i64)
    .bind(job.errors as i64)
    .bind(job.bytes_downloaded as i64)
    .bind(job.started_at)
    .bind(job.completed_at)
    .bind(job.crawl_rate)
    .bind(job.eta_seconds.map(|v| v as i64))
    .bind(&job.error_message)
    .execute(pool)
    .await;

    if let Err(e) = result {
        warn!(job_id = %job.job_id, error = %e, "Failed to update job in Postgres");
    }
}

/// Batch-update counters for dirty jobs in a single round-trip using `unnest` arrays.
pub async fn flush_job_counters(pool: &PgPool, snapshots: &[JobState]) {
    if snapshots.is_empty() {
        return;
    }

    let ids: Vec<&str> = snapshots.iter().map(|j| j.job_id.as_str()).collect();
    let statuses: Vec<&str> = snapshots.iter().map(|j| status_to_str(&j.status)).collect();
    let pages_crawled: Vec<i64> = snapshots.iter().map(|j| j.pages_crawled as i64).collect();
    let pages_indexed: Vec<i64> = snapshots.iter().map(|j| j.pages_indexed as i64).collect();
    let documents_sent: Vec<i64> = snapshots.iter().map(|j| j.documents_sent as i64).collect();
    let errors: Vec<i64> = snapshots.iter().map(|j| j.errors as i64).collect();
    let bytes_downloaded: Vec<i64> = snapshots
        .iter()
        .map(|j| j.bytes_downloaded as i64)
        .collect();
    let crawl_rates: Vec<f64> = snapshots.iter().map(|j| j.crawl_rate).collect();
    let eta_secs: Vec<Option<i64>> = snapshots
        .iter()
        .map(|j| j.eta_seconds.map(|v| v as i64))
        .collect();

    let result = sqlx::query(
        "UPDATE jobs AS j SET
            status = d.status,
            pages_crawled = d.pages_crawled,
            pages_indexed = d.pages_indexed,
            documents_sent = d.documents_sent,
            errors = d.errors,
            bytes_downloaded = d.bytes_downloaded,
            crawl_rate = d.crawl_rate,
            eta_seconds = d.eta_seconds
        FROM (
            SELECT * FROM unnest(
                $1::text[], $2::text[],
                $3::bigint[], $4::bigint[], $5::bigint[],
                $6::bigint[], $7::bigint[],
                $8::double precision[], $9::bigint[]
            ) AS t(
                job_id, status,
                pages_crawled, pages_indexed, documents_sent,
                errors, bytes_downloaded,
                crawl_rate, eta_seconds
            )
        ) AS d
        WHERE j.job_id = d.job_id",
    )
    .bind(&ids)
    .bind(&statuses)
    .bind(&pages_crawled)
    .bind(&pages_indexed)
    .bind(&documents_sent)
    .bind(&errors)
    .bind(&bytes_downloaded)
    .bind(&crawl_rates)
    .bind(&eta_secs)
    .execute(pool)
    .await;

    match result {
        Ok(r) => debug!(rows = r.rows_affected(), "Flushed job counters to Postgres"),
        Err(e) => warn!(error = %e, "Failed to flush job counters to Postgres"),
    }
}

// ============================================================================
// Reads
// ============================================================================

/// Load active (pending/running/paused) jobs for startup recovery.
pub async fn load_active_jobs(pool: &PgPool) -> Vec<JobState> {
    let rows = sqlx::query(
        "SELECT * FROM jobs WHERE status IN ('pending', 'running', 'paused') ORDER BY created_at",
    )
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => rows.iter().map(row_to_job_state).collect(),
        Err(e) => {
            warn!(error = %e, "Failed to load active jobs from Postgres");
            Vec::new()
        }
    }
}

/// Look up a single job by ID (fallback when not in HashMap).
pub async fn get_job_from_db(pool: &PgPool, job_id: &str) -> Option<JobState> {
    sqlx::query("SELECT * FROM jobs WHERE job_id = $1")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|row| row_to_job_state(&row))
}

/// Look up a single job scoped to an account.
pub async fn get_job_for_account(
    pool: &PgPool,
    job_id: &str,
    account_id: &str,
) -> Option<JobState> {
    let account_uuid: uuid::Uuid = account_id.parse().ok()?;
    sqlx::query("SELECT * FROM jobs WHERE job_id = $1 AND account_id = $2")
        .bind(job_id)
        .bind(account_uuid)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|row| row_to_job_state(&row))
}

/// Paginated list of all jobs, newest first.
pub async fn list_all_jobs_db(pool: &PgPool, limit: i64, offset: i64) -> Vec<JobState> {
    let rows = sqlx::query("SELECT * FROM jobs ORDER BY created_at DESC LIMIT $1 OFFSET $2")
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await;

    match rows {
        Ok(rows) => rows.iter().map(row_to_job_state).collect(),
        Err(e) => {
            warn!(error = %e, "Failed to list jobs from Postgres");
            Vec::new()
        }
    }
}

/// Paginated list of jobs for a specific account, newest first.
pub async fn list_jobs_for_account_db(
    pool: &PgPool,
    account_id: &str,
    limit: i64,
    offset: i64,
) -> Vec<JobState> {
    let account_uuid: uuid::Uuid = match account_id.parse() {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };
    let rows = sqlx::query(
        "SELECT * FROM jobs WHERE account_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(account_uuid)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => rows.iter().map(row_to_job_state).collect(),
        Err(e) => {
            warn!(error = %e, "Failed to list jobs for account from Postgres");
            Vec::new()
        }
    }
}
