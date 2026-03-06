//! Meilisearch engine registry.
//!
//! CRUD for saved Meilisearch instances stored in PostgreSQL,
//! with an endpoint to proxy index listing from each engine.

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use tracing::info;

use crate::auth::{get_user_account_id, AuthenticatedAccount, AuthenticatedUser};
use crate::{ApiError, AppState};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct EngineRecord {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub url: String,
    pub api_key: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateEngineRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEngineRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EngineIndex {
    pub uid: String,
    #[serde(rename = "primaryKey")]
    pub primary_key: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

/// Meilisearch /indexes response envelope
#[derive(Debug, Deserialize)]
struct MeilisearchIndexesResponse {
    results: Vec<MeilisearchIndexRaw>,
}

#[derive(Debug, Deserialize)]
struct MeilisearchIndexRaw {
    uid: String,
    #[serde(rename = "primaryKey")]
    primary_key: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

// ============================================================================
// Helpers
// ============================================================================

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

fn row_to_record(row: &sqlx::postgres::PgRow) -> EngineRecord {
    EngineRecord {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        account_id: row.get::<uuid::Uuid, _>("account_id").to_string(),
        name: row.get("name"),
        url: row.get("url"),
        api_key: row.get("api_key"),
        is_default: row.get("is_default"),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        updated_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .to_rfc3339(),
    }
}

fn get_pool(state: &AppState) -> Result<&sqlx::PgPool, ApiError> {
    state
        .db_pool
        .as_ref()
        .ok_or_else(|| ApiError::new("Database not configured", "internal_error"))
}

// ============================================================================
// CRUD Handlers
// ============================================================================

pub(crate) async fn create_engine(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Json(req): Json<CreateEngineRequest>,
) -> Result<(StatusCode, Json<EngineRecord>), ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    if req.name.trim().is_empty() {
        return Err(ApiError::new("Name is required", "validation_error"));
    }
    if req.url.trim().is_empty() {
        return Err(ApiError::new("URL is required", "validation_error"));
    }

    let api_key = req.api_key.as_deref().unwrap_or("");
    let is_default = req.is_default.unwrap_or(false);

    // If setting as default, unset any existing default first
    if is_default {
        let _ = sqlx::query(
            "UPDATE meilisearch_engines SET is_default = false WHERE account_id = $1 AND is_default = true",
        )
        .bind(account_id)
        .execute(pool)
        .await;
    }

    let row = sqlx::query(
        "INSERT INTO meilisearch_engines (account_id, name, url, api_key, is_default) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING *",
    )
    .bind(account_id)
    .bind(req.name.trim())
    .bind(req.url.trim())
    .bind(api_key)
    .bind(is_default)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("meilisearch_engines_account_id_name_key") {
                return ApiError::new("An engine with this name already exists", "conflict");
            }
        }
        ApiError::new(format!("Failed to create engine: {e}"), "internal_error")
    })?;

    let record = row_to_record(&row);
    info!(engine_id = %record.id, name = %record.name, "Meilisearch engine created");

    Ok((StatusCode::CREATED, Json(record)))
}

pub(crate) async fn list_engines(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
) -> Result<Json<Vec<EngineRecord>>, ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let rows = sqlx::query(
        "SELECT * FROM meilisearch_engines WHERE account_id = $1 ORDER BY is_default DESC, created_at DESC",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::new(format!("Failed to list engines: {e}"), "internal_error"))?;

    let records: Vec<EngineRecord> = rows.iter().map(row_to_record).collect();
    Ok(Json(records))
}

pub(crate) async fn get_engine(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(engine_id): Path<String>,
) -> Result<Json<EngineRecord>, ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let engine_uuid: uuid::Uuid = engine_id
        .parse()
        .map_err(|_| ApiError::new("Invalid engine ID", "validation_error"))?;

    let row = sqlx::query("SELECT * FROM meilisearch_engines WHERE id = $1 AND account_id = $2")
        .bind(engine_uuid)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
        .ok_or_else(|| ApiError::new("Engine not found", "not_found"))?;

    Ok(Json(row_to_record(&row)))
}

pub(crate) async fn update_engine(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(engine_id): Path<String>,
    Json(req): Json<UpdateEngineRequest>,
) -> Result<Json<EngineRecord>, ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let engine_uuid: uuid::Uuid = engine_id
        .parse()
        .map_err(|_| ApiError::new("Invalid engine ID", "validation_error"))?;

    let existing =
        sqlx::query("SELECT * FROM meilisearch_engines WHERE id = $1 AND account_id = $2")
            .bind(engine_uuid)
            .bind(account_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
            .ok_or_else(|| ApiError::new("Engine not found", "not_found"))?;

    let new_name = req.name.as_deref().unwrap_or_else(|| existing.get("name"));
    if new_name.trim().is_empty() {
        return Err(ApiError::new("Name cannot be empty", "validation_error"));
    }

    let new_url = req.url.as_deref().unwrap_or_else(|| existing.get("url"));
    if new_url.trim().is_empty() {
        return Err(ApiError::new("URL cannot be empty", "validation_error"));
    }

    let new_api_key: String = req
        .api_key
        .unwrap_or_else(|| existing.get::<String, _>("api_key"));

    let row = sqlx::query(
        "UPDATE meilisearch_engines SET name = $1, url = $2, api_key = $3 \
         WHERE id = $4 AND account_id = $5 RETURNING *",
    )
    .bind(new_name.trim())
    .bind(new_url.trim())
    .bind(&new_api_key)
    .bind(engine_uuid)
    .bind(account_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("meilisearch_engines_account_id_name_key") {
                return ApiError::new("An engine with this name already exists", "conflict");
            }
        }
        ApiError::new(format!("Failed to update engine: {e}"), "internal_error")
    })?;

    let record = row_to_record(&row);
    info!(engine_id = %record.id, name = %record.name, "Meilisearch engine updated");

    Ok(Json(record))
}

pub(crate) async fn delete_engine(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(engine_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let engine_uuid: uuid::Uuid = engine_id
        .parse()
        .map_err(|_| ApiError::new("Invalid engine ID", "validation_error"))?;

    let result = sqlx::query("DELETE FROM meilisearch_engines WHERE id = $1 AND account_id = $2")
        .bind(engine_uuid)
        .bind(account_id)
        .execute(pool)
        .await
        .map_err(|e| ApiError::new(format!("Failed to delete engine: {e}"), "internal_error"))?;

    if result.rows_affected() == 0 {
        return Err(ApiError::new("Engine not found", "not_found"));
    }

    info!(engine_id = %engine_id, "Meilisearch engine deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn set_default_engine(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(engine_id): Path<String>,
) -> Result<Json<EngineRecord>, ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let engine_uuid: uuid::Uuid = engine_id
        .parse()
        .map_err(|_| ApiError::new("Invalid engine ID", "validation_error"))?;

    // Verify engine exists for this account
    let _ = sqlx::query("SELECT id FROM meilisearch_engines WHERE id = $1 AND account_id = $2")
        .bind(engine_uuid)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
        .ok_or_else(|| ApiError::new("Engine not found", "not_found"))?;

    // Unset all defaults for this account
    sqlx::query(
        "UPDATE meilisearch_engines SET is_default = false WHERE account_id = $1 AND is_default = true",
    )
    .bind(account_id)
    .execute(pool)
    .await
    .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?;

    // Set new default
    let row = sqlx::query(
        "UPDATE meilisearch_engines SET is_default = true WHERE id = $1 AND account_id = $2 RETURNING *",
    )
    .bind(engine_uuid)
    .bind(account_id)
    .fetch_one(pool)
    .await
    .map_err(|e| ApiError::new(format!("Failed to set default: {e}"), "internal_error"))?;

    let record = row_to_record(&row);
    info!(engine_id = %record.id, name = %record.name, "Meilisearch engine set as default");

    Ok(Json(record))
}

pub(crate) async fn list_engine_indexes(
    State(state): State<Arc<AppState>>,
    account_ext: Option<Extension<AuthenticatedAccount>>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    Path(engine_id): Path<String>,
) -> Result<Json<Vec<EngineIndex>>, ApiError> {
    let pool = get_pool(&state)?;
    let account_id = resolve_account_id(
        pool,
        account_ext.as_ref().map(|e| &e.0),
        user_ext.as_ref().map(|e| &e.0),
    )
    .await?;

    let engine_uuid: uuid::Uuid = engine_id
        .parse()
        .map_err(|_| ApiError::new("Invalid engine ID", "validation_error"))?;

    let row = sqlx::query("SELECT * FROM meilisearch_engines WHERE id = $1 AND account_id = $2")
        .bind(engine_uuid)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| ApiError::new(format!("Database error: {e}"), "internal_error"))?
        .ok_or_else(|| ApiError::new("Engine not found", "not_found"))?;

    let engine_url: String = row.get("url");
    let engine_api_key: String = row.get("api_key");

    let client = reqwest::Client::new();
    let mut req = client.get(format!(
        "{}/indexes?limit=100",
        engine_url.trim_end_matches('/')
    ));
    if !engine_api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {engine_api_key}"));
    }

    let resp = req.send().await.map_err(|e| {
        ApiError::new(
            format!("Failed to connect to Meilisearch: {e}"),
            "bad_request",
        )
    })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(ApiError::new(
            format!("Meilisearch returned {status}: {body}"),
            "bad_request",
        ));
    }

    let ms_resp: MeilisearchIndexesResponse = resp.json().await.map_err(|e| {
        ApiError::new(
            format!("Failed to parse Meilisearch response: {e}"),
            "internal_error",
        )
    })?;

    let indexes: Vec<EngineIndex> = ms_resp
        .results
        .into_iter()
        .map(|idx| EngineIndex {
            uid: idx.uid,
            primary_key: idx.primary_key,
            created_at: idx.created_at,
            updated_at: idx.updated_at,
        })
        .collect();

    Ok(Json(indexes))
}
