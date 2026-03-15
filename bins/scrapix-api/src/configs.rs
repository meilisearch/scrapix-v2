//! Saved crawl configs with optional cron scheduling.
//!
//! CRUD for named crawl configurations stored in PostgreSQL,
//! triggerable on demand or via cron expressions.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use tracing::{error, info, warn};

use scrapix_core::CrawlConfig;

use crate::auth::{get_user_account_id, AuthenticatedAccount, AuthenticatedUser};
use crate::{do_create_crawl, AccountContext, ApiError, AppState};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct CrawlConfigRecord {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub config: serde_json::Value,
    pub cron_expression: Option<String>,
    pub cron_enabled: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_job_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateConfigRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub config: CrawlConfig,
    #[serde(default)]
    pub cron_expression: Option<String>,
    #[serde(default)]
    pub cron_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub config: Option<CrawlConfig>,
    #[serde(default)]
    pub cron_expression: Option<Option<String>>,
    #[serde(default)]
    pub cron_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListConfigsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Serialize)]
pub struct TriggerResponse {
    pub job_id: String,
    pub config_id: String,
    pub message: String,
}

// ============================================================================
// Helpers
// ============================================================================

/// Resolve account_id from either API key auth or session auth.
async fn resolve_account_id(
    pool: &PgPool,
    account_ext: Option<&AuthenticatedAccount>,
    user_ext: Option<&AuthenticatedUser>,
) -> Result<uuid::Uuid, ApiError> {
    if let Some(acct) = account_ext {
        acct.account_id
            .parse::<uuid::Uuid>()
            .map_err(|_| ApiError::new("Invalid account ID", "internal_error"))
    } else if let Some(user) = user_ext {
        get_user_account_id(pool, user.user_id)
            .await
            .map_err(|_| ApiError::new("Account not found", "not_found"))
    } else {
        Err(ApiError::new("Not authenticated", "unauthorized"))
    }
}

/// Compute the next run time from a cron expression.
pub(crate) fn compute_next_run(cron_expr: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    use croner::Cron;
    use std::str::FromStr;

    let cron = Cron::from_str(cron_expr).map_err(|e| format!("Invalid cron expression: {e}"))?;

    cron.find_next_occurrence(&chrono::Utc::now(), false)
        .map_err(|e| format!("Failed to compute next run: {e}"))
}

fn row_to_record(row: &sqlx::postgres::PgRow) -> CrawlConfigRecord {
    CrawlConfigRecord {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        account_id: row.get::<uuid::Uuid, _>("account_id").to_string(),
        name: row.get("name"),
        description: row.get("description"),
        config: row.get("config"),
        cron_expression: row.get("cron_expression"),
        cron_enabled: row.get("cron_enabled"),
        last_run_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_run_at")
            .map(|t| t.to_rfc3339()),
        next_run_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("next_run_at")
            .map(|t| t.to_rfc3339()),
        last_job_id: row.get("last_job_id"),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        updated_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .to_rfc3339(),
    }
}

// ============================================================================
// CRUD Handlers
// ============================================================================

pub(crate) async fn create_config(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Json(req): Json<CreateConfigRequest>,
) -> Result<(StatusCode, Json<CrawlConfigRecord>), ApiError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))?;

    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    if req.name.trim().is_empty() {
        return Err(ApiError::new("Name is required", "validation_error"));
    }

    // Validate cron expression if provided
    let next_run_at = if let Some(ref cron_expr) = req.cron_expression {
        if req.cron_enabled {
            Some(compute_next_run(cron_expr).map_err(|e| ApiError::new(e, "validation_error"))?)
        } else {
            // Parse to validate even if not enabled
            let _ =
                compute_next_run(cron_expr).map_err(|e| ApiError::new(e, "validation_error"))?;
            None
        }
    } else {
        None
    };

    let config_json = serde_json::to_value(&req.config)
        .map_err(|e| ApiError::new(format!("Invalid config: {e}"), "validation_error"))?;

    let row = sqlx::query(
        "INSERT INTO crawl_configs (account_id, name, description, config, cron_expression, cron_enabled, next_run_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING *",
    )
    .bind(account_id)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(&config_json)
    .bind(&req.cron_expression)
    .bind(req.cron_enabled)
    .bind(next_run_at)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("crawl_configs_account_id_name_key") {
                return ApiError::new(
                    "A config with this name already exists",
                    "conflict",
                );
            }
        }
        ApiError::new(format!("Failed to create config: {e}"), "internal_error")
    })?;

    let record = row_to_record(&row);
    info!(config_id = %record.id, name = %record.name, "Crawl config created");

    Ok((StatusCode::CREATED, Json(record)))
}

pub(crate) async fn list_configs(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Query(query): Query<ListConfigsQuery>,
) -> Result<Json<Vec<CrawlConfigRecord>>, ApiError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))?;

    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let rows = sqlx::query(
        "SELECT * FROM crawl_configs WHERE account_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(account_id)
    .bind(query.limit)
    .bind(query.offset)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::new(format!("Failed to list configs: {e}"), "internal_error"))?;

    let records: Vec<CrawlConfigRecord> = rows.iter().map(row_to_record).collect();
    Ok(Json(records))
}

pub(crate) async fn get_config(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(config_id): Path<String>,
) -> Result<Json<CrawlConfigRecord>, ApiError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))?;

    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let config_uuid: uuid::Uuid = config_id
        .parse()
        .map_err(|_| ApiError::new("Invalid config ID", "validation_error"))?;

    let row = sqlx::query("SELECT * FROM crawl_configs WHERE id = $1 AND account_id = $2")
        .bind(config_uuid)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
        .ok_or_else(|| ApiError::new("Config not found", "not_found"))?;

    Ok(Json(row_to_record(&row)))
}

pub(crate) async fn update_config(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(config_id): Path<String>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<CrawlConfigRecord>, ApiError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))?;

    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let config_uuid: uuid::Uuid = config_id
        .parse()
        .map_err(|_| ApiError::new("Invalid config ID", "validation_error"))?;

    // Fetch existing record to merge updates
    let existing = sqlx::query("SELECT * FROM crawl_configs WHERE id = $1 AND account_id = $2")
        .bind(config_uuid)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
        .ok_or_else(|| ApiError::new("Config not found", "not_found"))?;

    let new_name = req.name.as_deref().unwrap_or_else(|| existing.get("name"));
    if new_name.trim().is_empty() {
        return Err(ApiError::new("Name cannot be empty", "validation_error"));
    }

    let new_description: Option<String> = match req.description {
        Some(d) => d,
        None => existing.get("description"),
    };

    let new_config: serde_json::Value = if let Some(ref cfg) = req.config {
        serde_json::to_value(cfg)
            .map_err(|e| ApiError::new(format!("Invalid config: {e}"), "validation_error"))?
    } else {
        existing.get("config")
    };

    let new_cron_expression: Option<String> = match req.cron_expression {
        Some(expr) => expr,
        None => existing.get("cron_expression"),
    };

    let new_cron_enabled = req
        .cron_enabled
        .unwrap_or_else(|| existing.get("cron_enabled"));

    // Recompute next_run_at
    let new_next_run_at = if new_cron_enabled {
        if let Some(ref expr) = new_cron_expression {
            Some(compute_next_run(expr).map_err(|e| ApiError::new(e, "validation_error"))?)
        } else {
            None
        }
    } else {
        None
    };

    let row = sqlx::query(
        "UPDATE crawl_configs SET name = $1, description = $2, config = $3, \
         cron_expression = $4, cron_enabled = $5, next_run_at = $6 \
         WHERE id = $7 AND account_id = $8 RETURNING *",
    )
    .bind(new_name.trim())
    .bind(&new_description)
    .bind(&new_config)
    .bind(&new_cron_expression)
    .bind(new_cron_enabled)
    .bind(new_next_run_at)
    .bind(config_uuid)
    .bind(account_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("crawl_configs_account_id_name_key") {
                return ApiError::new("A config with this name already exists", "conflict");
            }
        }
        ApiError::new(format!("Failed to update config: {e}"), "internal_error")
    })?;

    let record = row_to_record(&row);
    info!(config_id = %record.id, name = %record.name, "Crawl config updated");

    Ok(Json(record))
}

pub(crate) async fn delete_config(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(config_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))?;

    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let config_uuid: uuid::Uuid = config_id
        .parse()
        .map_err(|_| ApiError::new("Invalid config ID", "validation_error"))?;

    let result = sqlx::query("DELETE FROM crawl_configs WHERE id = $1 AND account_id = $2")
        .bind(config_uuid)
        .bind(account_id)
        .execute(pool)
        .await
        .map_err(|e| ApiError::new(format!("Failed to delete config: {e}"), "internal_error"))?;

    if result.rows_affected() == 0 {
        return Err(ApiError::new("Config not found", "not_found"));
    }

    info!(config_id = %config_id, "Crawl config deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Trigger Handler
// ============================================================================

pub(crate) async fn trigger_config(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(config_id): Path<String>,
) -> Result<Json<TriggerResponse>, ApiError> {
    let pool = state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))?;

    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let config_uuid: uuid::Uuid = config_id
        .parse()
        .map_err(|_| ApiError::new("Invalid config ID", "validation_error"))?;

    let row = sqlx::query("SELECT * FROM crawl_configs WHERE id = $1 AND account_id = $2")
        .bind(config_uuid)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
        .ok_or_else(|| ApiError::new("Config not found", "not_found"))?;

    let config_json: serde_json::Value = row.get("config");
    let crawl_config: CrawlConfig = serde_json::from_value(config_json)
        .map_err(|e| ApiError::new(format!("Invalid stored config: {e}"), "internal_error"))?;

    let account_ctx = AccountContext {
        account_id: account_id.to_string(),
        api_key_id: None,
    };

    let response = do_create_crawl(&state, crawl_config, Some(&account_ctx)).await?;

    // Update last_run_at and last_job_id
    let _ =
        sqlx::query("UPDATE crawl_configs SET last_run_at = now(), last_job_id = $1 WHERE id = $2")
            .bind(&response.job_id)
            .bind(config_uuid)
            .execute(pool)
            .await;

    info!(
        config_id = %config_id,
        job_id = %response.job_id,
        "Crawl triggered from saved config"
    );

    Ok(Json(TriggerResponse {
        job_id: response.job_id,
        config_id: config_id.to_string(),
        message: "Crawl triggered successfully".to_string(),
    }))
}

// ============================================================================
// Cron Scheduler
// ============================================================================

/// Spawn the cron scheduler background task.
/// Checks every 30 seconds for configs whose next_run_at has passed.
pub(crate) fn spawn_cron_scheduler(
    state: Arc<AppState>,
    pool: PgPool,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = run_cron_tick(&state, &pool).await {
                        warn!(error = %e, "Cron scheduler tick failed");
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("Cron scheduler shutting down");
                    break;
                }
            }
        }
    })
}

async fn run_cron_tick(state: &Arc<AppState>, pool: &PgPool) -> Result<(), sqlx::Error> {
    // Fetch due configs with row-level locking
    let rows = sqlx::query(
        "SELECT * FROM crawl_configs \
         WHERE cron_enabled = true AND cron_expression IS NOT NULL AND next_run_at <= now() \
         FOR UPDATE SKIP LOCKED \
         LIMIT 50",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    info!(count = rows.len(), "Cron scheduler: processing due configs");

    for row in &rows {
        let config_id: uuid::Uuid = row.get("id");
        let config_name: String = row.get("name");
        let cron_expr: String = row.get("cron_expression");
        let config_json: serde_json::Value = row.get("config");
        let account_id: uuid::Uuid = row.get("account_id");

        // Deserialize crawl config
        let crawl_config: CrawlConfig = match serde_json::from_value(config_json) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    config_id = %config_id,
                    name = %config_name,
                    error = %e,
                    "Failed to deserialize stored config, disabling cron"
                );
                let _ = sqlx::query("UPDATE crawl_configs SET cron_enabled = false WHERE id = $1")
                    .bind(config_id)
                    .execute(pool)
                    .await;
                continue;
            }
        };

        let account_ctx = AccountContext {
            account_id: account_id.to_string(),
            api_key_id: None,
        };

        // Trigger crawl
        match do_create_crawl(state, crawl_config, Some(&account_ctx)).await {
            Ok(response) => {
                // Compute next run
                let next_run = compute_next_run(&cron_expr).ok();

                let _ = sqlx::query(
                    "UPDATE crawl_configs SET last_run_at = now(), last_job_id = $1, next_run_at = $2 WHERE id = $3",
                )
                .bind(&response.job_id)
                .bind(next_run)
                .bind(config_id)
                .execute(pool)
                .await;

                info!(
                    config_id = %config_id,
                    name = %config_name,
                    job_id = %response.job_id,
                    "Cron: crawl triggered"
                );
            }
            Err(e) => {
                warn!(
                    config_id = %config_id,
                    name = %config_name,
                    error = ?e,
                    "Cron: failed to trigger crawl, advancing next_run_at"
                );

                // Advance next_run_at to avoid retry storm
                let next_run = compute_next_run(&cron_expr).ok();
                let _ = sqlx::query("UPDATE crawl_configs SET next_run_at = $1 WHERE id = $2")
                    .bind(next_run)
                    .bind(config_id)
                    .execute(pool)
                    .await;
            }
        }
    }

    Ok(())
}
