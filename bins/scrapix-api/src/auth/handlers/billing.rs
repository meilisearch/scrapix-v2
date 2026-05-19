use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use sqlx::Row;
use std::sync::Arc;
use tracing::{error, info};

use super::{
    err, get_user_account_id, get_user_role, require_role, ApiError, AutoTopupRequest,
    BillingResponse, ErrorBody, MessageResponse, SpendLimitRequest, TopupRequest, TopupResponse,
    TransactionResponse, TransactionsListResponse, UpdateBillingRequest,
};
use crate::auth::{AuthState, AuthenticatedUser};

#[utoipa::path(
    get,
    path = "/account/billing",
    tag = "auth",
    responses(
        (status = 200, description = "Billing information", body = BillingResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_billing(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<BillingResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let row = sqlx::query(
        "SELECT tier, stripe_customer_id, credits_balance, \
         auto_topup_enabled, auto_topup_amount, auto_topup_threshold, monthly_spend_limit \
         FROM accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    Ok(Json(BillingResponse {
        tier: row.get("tier"),
        stripe_customer_id: row.get("stripe_customer_id"),
        credits_balance: row.get("credits_balance"),
        auto_topup_enabled: row.get("auto_topup_enabled"),
        auto_topup_amount: row.get("auto_topup_amount"),
        auto_topup_threshold: row.get("auto_topup_threshold"),
        monthly_spend_limit: row.get("monthly_spend_limit"),
    }))
}

#[utoipa::path(
    patch,
    path = "/account/billing",
    tag = "auth",
    request_body = UpdateBillingRequest,
    responses(
        (status = 200, description = "Billing tier updated", body = MessageResponse),
        (status = 400, description = "Invalid tier", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_billing(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdateBillingRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let valid_tiers = ["free", "starter", "pro", "enterprise"];
    if !valid_tiers.contains(&req.tier.as_str()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Invalid tier",
            "validation_error",
        ));
    }

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners can change billing tier
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner"])?;

    sqlx::query("UPDATE accounts SET tier = $1 WHERE id = $2")
        .bind(&req.tier)
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

    Ok(Json(MessageResponse {
        message: "Tier updated".to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/account/billing/topup",
    tag = "auth",
    request_body = TopupRequest,
    responses(
        (status = 200, description = "Credits topped up", body = TopupResponse),
        (status = 400, description = "Invalid amount", body = ErrorBody),
        (status = 403, description = "Spend limit exceeded", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn topup_credits(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<TopupRequest>,
) -> Result<Json<TopupResponse>, ApiError> {
    if req.amount <= 0 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Amount must be positive",
            "validation_error",
        ));
    }

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|e| {
            error!(user_id = %user.user_id, "topup: failed to get account_id: {e:?}");
            err(StatusCode::NOT_FOUND, "Account not found", "not_found")
        })?;

    // Check monthly spend limit
    scrapix_billing::check_spend_limit(&state.pool, account_id, req.amount)
        .await
        .map_err(|e| err(StatusCode::FORBIDDEN, &e.to_string(), e.code()))?;

    let mut tx = state.pool.begin().await.map_err(|e| {
        error!(account_id = %account_id, "topup: failed to begin transaction: {e}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let new_balance: i64 = sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance + $1 WHERE id = $2 RETURNING credits_balance",
    )
    .bind(req.amount)
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!(account_id = %account_id, amount = req.amount, "topup: failed to update balance: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update balance", "internal_error")
    })?;

    let tx_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'manual_topup', $2, $3, 'Manual credit top-up') RETURNING id",
    )
    .bind(account_id)
    .bind(req.amount)
    .bind(new_balance)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!(account_id = %account_id, "topup: failed to insert transaction: {e}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to log transaction",
            "internal_error",
        )
    })?;

    tx.commit().await.map_err(|e| {
        error!(account_id = %account_id, "topup: failed to commit: {e}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    info!(account_id = %account_id, amount = req.amount, new_balance, "Manual credit top-up");

    Ok(Json(TopupResponse {
        credits_balance: new_balance,
        transaction_id: tx_id.to_string(),
        message: format!("Added {} credits", req.amount),
    }))
}

#[utoipa::path(
    patch,
    path = "/account/billing/auto-topup",
    tag = "auth",
    request_body = AutoTopupRequest,
    responses(
        (status = 200, description = "Auto top-up settings updated", body = MessageResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_auto_topup(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<AutoTopupRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    if req.enabled {
        let amount = req.amount.unwrap_or(5000);
        let threshold = req.threshold.unwrap_or(500);
        if amount <= 0 || threshold < 0 {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "Amount must be positive and threshold non-negative",
                "validation_error",
            ));
        }
        sqlx::query(
            "UPDATE accounts SET auto_topup_enabled = true, auto_topup_amount = $1, auto_topup_threshold = $2 WHERE id = $3",
        )
        .bind(amount)
        .bind(threshold)
        .bind(account_id)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update", "internal_error")
        })?;
    } else {
        sqlx::query("UPDATE accounts SET auto_topup_enabled = false WHERE id = $1")
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
        message: if req.enabled {
            "Auto top-up enabled".to_string()
        } else {
            "Auto top-up disabled".to_string()
        },
    }))
}

#[utoipa::path(
    patch,
    path = "/account/billing/spend-limit",
    tag = "auth",
    request_body = SpendLimitRequest,
    responses(
        (status = 200, description = "Spend limit updated", body = MessageResponse),
        (status = 400, description = "Invalid limit", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_spend_limit(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<SpendLimitRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    if let Some(limit) = req.monthly_spend_limit {
        if limit <= 0 {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "Spend limit must be positive",
                "validation_error",
            ));
        }
    }

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    sqlx::query("UPDATE accounts SET monthly_spend_limit = $1 WHERE id = $2")
        .bind(req.monthly_spend_limit)
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

    Ok(Json(MessageResponse {
        message: match req.monthly_spend_limit {
            Some(limit) => format!("Monthly spend limit set to {}", limit),
            None => "Monthly spend limit removed".to_string(),
        },
    }))
}

#[utoipa::path(
    get,
    path = "/account/billing/transactions",
    tag = "auth",
    params(
        ("limit" = Option<i64>, Query, description = "Maximum number of transactions to return (default 50, max 200)"),
        ("offset" = Option<i64>, Query, description = "Offset for pagination (default 0)"),
    ),
    responses(
        (status = 200, description = "List of transactions", body = TransactionsListResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn list_transactions(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<TransactionsListResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let limit: i64 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(200);
    let offset: i64 = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?;

    let rows = sqlx::query(
        "SELECT id, type, amount, balance_after, description, created_at \
         FROM transactions WHERE account_id = $1 \
         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(account_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let transactions: Vec<TransactionResponse> = rows
        .iter()
        .map(|r| TransactionResponse {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            tx_type: r.get("type"),
            amount: r.get("amount"),
            balance_after: r.get("balance_after"),
            description: r.get("description"),
            created_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        })
        .collect();

    Ok(Json(TransactionsListResponse {
        transactions,
        total,
    }))
}
