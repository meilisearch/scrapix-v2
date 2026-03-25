use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tracing::info;

use super::{
    err, get_user_account_id, get_user_role, require_role, ApiError, ApiKeyResponse,
    CreateApiKeyRequest, CreatedApiKeyResponse, ErrorBody, MessageResponse,
};
use crate::auth::{AuthState, AuthenticatedUser};

#[utoipa::path(
    get,
    path = "/account/api-keys",
    tag = "auth",
    responses(
        (status = 200, description = "List of API keys", body = Vec<ApiKeyResponse>),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn list_api_keys(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<ApiKeyResponse>>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let rows = sqlx::query(
        "SELECT id, name, prefix, active, last_used_at, created_at \
         FROM api_keys WHERE account_id = $1 ORDER BY created_at DESC",
    )
    .bind(account_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let keys: Vec<ApiKeyResponse> = rows
        .iter()
        .map(|r| ApiKeyResponse {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            name: r.get("name"),
            prefix: r.get("prefix"),
            active: r.get("active"),
            last_used_at: r
                .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at")
                .map(|t| t.to_rfc3339()),
            created_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        })
        .collect();

    Ok(Json(keys))
}

#[utoipa::path(
    post,
    path = "/account/api-keys",
    tag = "auth",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 200, description = "API key created", body = CreatedApiKeyResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn create_api_key(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<CreatedApiKeyResponse>, ApiError> {
    if req.name.trim().is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Name is required",
            "validation_error",
        ));
    }

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners and admins can create API keys
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner", "admin"])?;

    // Generate key server-side (scoped to drop !Send ThreadRng before await)
    let (api_key, prefix, key_hash) = {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let random_part: String = (0..32)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
            .collect();
        let api_key = format!("sk_live_{}", random_part);
        let prefix = format!("{}...", &api_key[..12]);

        let mut hasher = Sha256::new();
        hasher.update(api_key.as_bytes());
        let key_hash = hex::encode(hasher.finalize());
        (api_key, prefix, key_hash)
    };

    let key_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO api_keys (account_id, name, prefix, key_hash) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(account_id)
    .bind(req.name.trim())
    .bind(&prefix)
    .bind(&key_hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create key",
            "internal_error",
        )
    })?;

    info!(key_id = %key_id, account_id = %account_id, "API key created");

    Ok(Json(CreatedApiKeyResponse {
        id: key_id.to_string(),
        name: req.name,
        prefix,
        key: api_key,
    }))
}

#[utoipa::path(
    patch,
    path = "/account/api-keys/{id}",
    tag = "auth",
    params(
        ("id" = String, Path, description = "API key ID to revoke"),
    ),
    responses(
        (status = 200, description = "API key revoked", body = MessageResponse),
        (status = 400, description = "Invalid key ID", body = ErrorBody),
        (status = 404, description = "Key not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn revoke_api_key(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(key_id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners and admins can revoke API keys
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner", "admin"])?;

    let key_uuid: uuid::Uuid = key_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid key ID",
            "validation_error",
        )
    })?;

    let result =
        sqlx::query("UPDATE api_keys SET active = false WHERE id = $1 AND account_id = $2")
            .bind(key_uuid)
            .bind(account_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to revoke key",
                    "internal_error",
                )
            })?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Key not found", "not_found"));
    }

    Ok(Json(MessageResponse {
        message: "Key revoked".to_string(),
    }))
}
