use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use scrapix_billing_hyperline::HyperlineClient;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tracing::{error, info};

use super::{
    err, get_user_account_id, get_user_role, require_role, ApiError, AutoTopupRequest,
    BillingResponse, ErrorBody, MessageResponse, PortalResponse, SpendLimitRequest, TopupRequest,
    TopupResponse, TransactionResponse, TransactionsListResponse, UpdateBillingRequest,
};
use crate::auth::{AuthState, AuthenticatedUser};

/// Lazily link an account to a Hyperline customer.
///
/// Looks up `(account_name, owner_email)`, queries Hyperline for an
/// existing customer with `external_id = account_id` (recovers from
/// crashes that orphaned a customer mid-link), creates one if absent,
/// then `UPDATE accounts SET hyperline_customer_id = COALESCE(...)` so
/// concurrent linkers converge on a single value.
///
/// Returns the linked customer id. Network/auth failures bubble up;
/// the caller decides which HTTP status to map to.
pub(crate) async fn link_account_to_hyperline(
    pool: &PgPool,
    client: &HyperlineClient,
    account_id: uuid::Uuid,
    account_name: &str,
    owner_email: &str,
) -> Result<String, scrapix_billing_hyperline::HyperlineError> {
    let external_id = account_id.to_string();

    let customer = match client.find_customer_by_external_id(&external_id).await? {
        Some(existing) => {
            info!(
                account_id = %account_id,
                customer_id = %existing.id,
                "hyperline: recovered orphaned customer by external_id"
            );
            existing
        }
        None => {
            let created = client
                .create_customer(&external_id, account_name, owner_email)
                .await?;
            info!(
                account_id = %account_id,
                customer_id = %created.id,
                "hyperline: created customer"
            );
            created
        }
    };

    // COALESCE handles the race where another request linked first; we
    // adopt whatever id won and discard our just-created one (it stays
    // in Hyperline as an orphan, but `find_customer_by_external_id`
    // will reuse it on any future link attempt for this account).
    let linked_id: String = sqlx::query_scalar(
        "UPDATE accounts \
         SET hyperline_customer_id = COALESCE(hyperline_customer_id, $1) \
         WHERE id = $2 RETURNING hyperline_customer_id",
    )
    .bind(&customer.id)
    .bind(account_id)
    .fetch_one(pool)
    .await
    .map_err(|e| scrapix_billing_hyperline::HyperlineError::InvalidConfig(format!("db: {e}")))?;

    Ok(linked_id)
}

/// Owner email for an account — used as the Hyperline `email` field
/// on lazy customer creation. Picks the oldest 'owner' membership so
/// the result is stable across reruns.
pub(crate) async fn account_owner_email(
    pool: &PgPool,
    account_id: uuid::Uuid,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT a.name AS account_name, u.email AS owner_email \
         FROM accounts a \
         JOIN account_members m ON m.account_id = a.id \
         JOIN users u ON u.id = m.user_id \
         WHERE a.id = $1 AND m.role = 'owner' \
         ORDER BY m.joined_at ASC \
         LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("account_name"), r.get("owner_email"))))
}

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

    // Balance-source-of-truth is still the local ledger (hot path). The
    // Hyperline handles are returned so the console can deep-link to the
    // hosted portal; a live wallet-balance read is a follow-up once the
    // reconcile worker is in place.
    let row = sqlx::query(
        "SELECT tier, credits_balance, monthly_spend_limit, \
         hyperline_customer_id, hyperline_wallet_id, payment_method_status \
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

    let hyperline_customer_id: Option<String> = row.get("hyperline_customer_id");
    let hyperline_wallet_id: Option<String> = row.get("hyperline_wallet_id");

    // Best-effort live wallet read. Failures (no Hyperline client, not
    // yet linked, network error, 404) all fall through to None — the
    // local `credits_balance` is still authoritative for the ledger.
    let (hyperline_wallet_balance, hyperline_wallet_currency) = match (
        state.hyperline_client.as_ref(),
        hyperline_wallet_id.as_deref(),
    ) {
        (Some(client), Some(wallet_id)) => match client.get_wallet(wallet_id).await {
            Ok(wallet) => (Some(wallet.balance.amount), wallet.currency),
            Err(e) => {
                info!(
                    account_id = %account_id,
                    wallet_id = %wallet_id,
                    error = %e,
                    "hyperline live wallet read failed — returning local balance only"
                );
                (None, None)
            }
        },
        _ => (None, None),
    };

    Ok(Json(BillingResponse {
        tier: row.get("tier"),
        credits_balance: row.get("credits_balance"),
        monthly_spend_limit: row.get("monthly_spend_limit"),
        hyperline_customer_id,
        hyperline_wallet_id,
        payment_method_status: row.get("payment_method_status"),
        hyperline_wallet_balance,
        hyperline_wallet_currency,
    }))
}

/// `GET /account/billing/portal` — exchange for a Hyperline hosted-portal URL.
///
/// Replaces the old Stripe SetupIntent + 3DS payment-method UI. Users
/// manage cards, invoices, and auto-recharge from the hosted portal;
/// we just fetch the per-customer URL and redirect.
///
/// Returns 404 if the account hasn't been linked to Hyperline yet
/// (expected during the customer-backfill window), 503 if the
/// Hyperline REST client isn't configured, or 502 on an upstream
/// failure.
#[utoipa::path(
    get,
    path = "/account/billing/portal",
    tag = "auth",
    responses(
        (status = 200, description = "Hosted-portal URL", body = PortalResponse),
        (status = 404, description = "Account not linked to Hyperline", body = ErrorBody),
        (status = 502, description = "Hyperline upstream error", body = ErrorBody),
        (status = 503, description = "Hyperline client not configured", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_billing_portal(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<PortalResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let Some(client) = state.hyperline_client.as_ref() else {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Hyperline client not configured",
            "hyperline_disabled",
        ));
    };

    let customer_id: Option<String> =
        sqlx::query_scalar("SELECT hyperline_customer_id FROM accounts WHERE id = $1")
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
            .flatten();

    let customer_id = match customer_id {
        Some(id) => id,
        None => {
            // First portal request for this account — lazy-link to Hyperline.
            // Closes the gap left by SCR-68 (no automated customer creation):
            // accounts created before the migration, or whose eager-create at
            // signup failed, would otherwise be stuck with a 404 forever.
            let Some((account_name, owner_email)) = account_owner_email(&state.pool, account_id)
                .await
                .map_err(|e| {
                    error!(account_id = %account_id, error = %e, "owner lookup failed");
                    err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database error",
                        "internal_error",
                    )
                })?
            else {
                return Err(err(
                    StatusCode::NOT_FOUND,
                    "Account has no owner — cannot link to Hyperline",
                    "no_owner",
                ));
            };

            link_account_to_hyperline(&state.pool, client, account_id, &account_name, &owner_email)
                .await
                .map_err(|e| {
                    error!(
                        account_id = %account_id,
                        error = %e,
                        "hyperline lazy-link failed"
                    );
                    err(
                        StatusCode::BAD_GATEWAY,
                        "Failed to link account to Hyperline",
                        "hyperline_upstream",
                    )
                })?
        }
    };

    match client.get_portal_url(&customer_id).await {
        Ok(link) => Ok(Json(PortalResponse { url: link.url })),
        Err(e) => {
            error!(
                account_id = %account_id,
                customer_id = %customer_id,
                error = %e,
                "hyperline portal URL fetch failed"
            );
            Err(err(
                StatusCode::BAD_GATEWAY,
                "Hyperline portal URL unavailable",
                "hyperline_upstream",
            ))
        }
    }
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
        (status = 410, description = "Moved to Hyperline hosted portal", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
/// Auto top-up moved to Hyperline wallet rules — this endpoint is a stub
/// that returns 410 Gone so the frontend knows to redirect to the hosted
/// portal instead of POSTing here.
pub(crate) async fn update_auto_topup(
    State(_state): State<Arc<AuthState>>,
    Extension(_user): Extension<AuthenticatedUser>,
    Json(_req): Json<AutoTopupRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    Err(err(
        StatusCode::GONE,
        "Auto top-up is now configured on the Hyperline hosted portal. \
         Use GET /account/billing/portal to obtain a portal session.",
        "moved_to_hyperline",
    ))
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
