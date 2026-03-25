use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use sqlx::Row;
use std::sync::Arc;
use tracing::info;

use super::{
    err, get_user_account_id, get_user_role, require_role, AccountListItem, AccountResponse,
    ApiError, CreateAccountRequest, ErrorBody, MessageResponse, UpdateAccountRequest,
    UpdateMeRequest, UserResponse,
};
use crate::auth::{AuthState, AuthenticatedUser};

#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Current user info", body = UserResponse),
        (status = 404, description = "User not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_me(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<UserResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT id, email, full_name, email_verified, notify_job_emails FROM users WHERE id = $1",
    )
    .bind(user.user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "User not found", "not_found"))?;

    // If user has a selected_account_id, use that; otherwise default to first
    let account = if let Some(selected_id) = user.selected_account_id {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 AND m.account_id = $2",
        )
        .bind(user.user_id)
        .bind(selected_id)
        .fetch_optional(&state.pool)
        .await
    } else {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 LIMIT 1",
        )
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
    }
    .ok()
    .flatten()
    .map(|r| AccountResponse {
        id: r.get::<uuid::Uuid, _>("id").to_string(),
        name: r.get("name"),
        tier: r.get("tier"),
        active: r.get("active"),
        role: r.get("role"),
        credits_balance: r.get("credits_balance"),
    });

    Ok(Json(UserResponse {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        email: row.get("email"),
        full_name: row.get("full_name"),
        email_verified: row.get("email_verified"),
        notify_job_emails: row.get("notify_job_emails"),
        account,
    }))
}

#[utoipa::path(
    patch,
    path = "/auth/me",
    tag = "auth",
    request_body = UpdateMeRequest,
    responses(
        (status = 200, description = "User updated", body = MessageResponse),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_me(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdateMeRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    if let Some(ref name) = req.full_name {
        sqlx::query("UPDATE users SET full_name = $1 WHERE id = $2")
            .bind(name)
            .bind(user.user_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update",
                    "internal_error",
                )
            })?;
    }
    if let Some(notify) = req.notify_job_emails {
        sqlx::query("UPDATE users SET notify_job_emails = $1 WHERE id = $2")
            .bind(notify)
            .bind(user.user_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update",
                    "internal_error",
                )
            })?;
    }
    Ok(Json(MessageResponse {
        message: "Updated".to_string(),
    }))
}

#[utoipa::path(
    get,
    path = "/account",
    tag = "auth",
    responses(
        (status = 200, description = "Account details", body = AccountResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<AccountResponse>, ApiError> {
    let row = if let Some(selected_id) = user.selected_account_id {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 AND m.account_id = $2",
        )
        .bind(user.user_id)
        .bind(selected_id)
        .fetch_optional(&state.pool)
        .await
    } else {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 LIMIT 1",
        )
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
    }
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    Ok(Json(AccountResponse {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        name: row.get("name"),
        tier: row.get("tier"),
        active: row.get("active"),
        role: row.get("role"),
        credits_balance: row.get("credits_balance"),
    }))
}

#[utoipa::path(
    patch,
    path = "/account",
    tag = "auth",
    request_body = UpdateAccountRequest,
    responses(
        (status = 200, description = "Account updated", body = MessageResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners can update account settings
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner"])?;

    if let Some(ref name) = req.name {
        sqlx::query("UPDATE accounts SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(account_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update",
                    "internal_error",
                )
            })?;
    }
    Ok(Json(MessageResponse {
        message: "Updated".to_string(),
    }))
}

/// GET /auth/me/accounts -- list all accounts the user belongs to
pub(crate) async fn list_my_accounts(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<AccountListItem>>, ApiError> {
    let rows = sqlx::query(
        "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
         FROM account_members m JOIN accounts a ON a.id = m.account_id \
         WHERE m.user_id = $1 ORDER BY m.joined_at ASC",
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let accounts: Vec<AccountListItem> = rows
        .iter()
        .map(|r| AccountListItem {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            name: r.get("name"),
            tier: r.get("tier"),
            active: r.get("active"),
            role: r.get("role"),
            credits_balance: r.get("credits_balance"),
        })
        .collect();

    Ok(Json(accounts))
}

/// POST /auth/me/accounts -- create a new account (user becomes owner)
pub(crate) async fn create_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<AccountListItem>), ApiError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Account name is required",
            "validation_error",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let account_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO accounts (name) VALUES ($1) RETURNING id")
            .bind(&name)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to create account",
                    "internal_error",
                )
            })?;

    sqlx::query("INSERT INTO account_members (user_id, account_id, role) VALUES ($1, $2, 'owner')")
        .bind(user.user_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create membership",
                "internal_error",
            )
        })?;

    // Log the initial credit deposit
    sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'initial_deposit', 100, 100, 'Welcome credit deposit')",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to log initial deposit",
            "internal_error",
        )
    })?;

    tx.commit().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    info!(user_id = %user.user_id, account_id = %account_id, name = %name, "New account created");

    Ok((
        StatusCode::CREATED,
        Json(AccountListItem {
            id: account_id.to_string(),
            name,
            tier: "free".to_string(),
            active: true,
            role: "owner".to_string(),
            credits_balance: 100,
        }),
    ))
}
