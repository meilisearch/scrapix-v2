//! Stripe payment integration.
//!
//! Handles customer creation, payment methods, credit purchases via PaymentIntents,
//! and webhook processing. All UI is custom — Stripe is used purely as a backend
//! payment engine.

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use stripe::{
    Client as StripeClient, CreateCustomer, CreatePaymentIntent, CreateSetupIntent, Currency,
    Customer, CustomerId, EventObject, EventType, ListPaymentMethods, PaymentIntent,
    PaymentIntentStatus, PaymentMethod, PaymentMethodId, PaymentMethodTypeFilter, SetupIntent,
    Webhook,
};
use tracing::{error, info, warn};

use crate::auth::{AuthState, AuthenticatedUser};

// ============================================================================
// Credit pack definitions
// ============================================================================

/// Credit packs available for purchase. Amount in cents (USD).
const CREDIT_PACKS: &[(i64, i64)] = &[
    (1_000, 1_000),   // 1,000 credits = $10.00
    (5_000, 4_000),   // 5,000 credits = $40.00
    (10_000, 7_000),  // 10,000 credits = $70.00
    (50_000, 25_000), // 50,000 credits = $250.00
];

fn price_for_credits(credits: i64) -> Option<i64> {
    CREDIT_PACKS
        .iter()
        .find(|(c, _)| *c == credits)
        .map(|(_, price)| *price)
}

// ============================================================================
// State
// ============================================================================

/// Shared Stripe state, injected into routes as an Extension.
#[derive(Clone)]
pub struct StripeState {
    pub client: StripeClient,
    pub webhook_secret: Option<String>,
}

impl StripeState {
    pub fn new(secret_key: &str, webhook_secret: Option<String>) -> Self {
        Self {
            client: StripeClient::new(secret_key),
            webhook_secret,
        }
    }
}

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Serialize)]
pub(crate) struct SetupIntentResponse {
    client_secret: String,
}

#[derive(Serialize)]
pub(crate) struct PaymentMethodResponse {
    id: String,
    brand: Option<String>,
    last4: Option<String>,
    exp_month: Option<i32>,
    exp_year: Option<i32>,
    is_default: bool,
}

#[derive(Deserialize)]
pub struct PurchaseCreditsRequest {
    credits: i64,
    payment_method_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PurchaseResponse {
    status: String,
    client_secret: Option<String>,
    credits: i64,
    amount_cents: i64,
    message: String,
}

#[derive(Deserialize)]
pub struct SetDefaultPaymentMethodRequest {
    payment_method_id: String,
}

#[derive(Serialize)]
pub(crate) struct MessageResponse {
    message: String,
}

type ApiError = (StatusCode, Json<StripeErrorBody>);

#[derive(Debug, Serialize)]
pub(crate) struct StripeErrorBody {
    error: String,
    code: String,
}

fn err(status: StatusCode, msg: &str, code: &str) -> ApiError {
    (
        status,
        Json(StripeErrorBody {
            error: msg.to_string(),
            code: code.to_string(),
        }),
    )
}

// ============================================================================
// Helpers
// ============================================================================

/// Get or create a Stripe customer for the given account.
async fn get_or_create_customer(
    stripe: &StripeClient,
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
) -> Result<CustomerId, ApiError> {
    // Check if we already have a stripe_customer_id
    let existing: Option<String> =
        sqlx::query_scalar("SELECT stripe_customer_id FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to query stripe_customer_id");
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error",
                    "internal_error",
                )
            })?
            .flatten();

    if let Some(cid) = existing {
        return cid.parse::<CustomerId>().map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid stripe customer ID in database",
                "internal_error",
            )
        });
    }

    // Fetch account name and email for the customer
    let row = sqlx::query(
        "SELECT a.name, u.email FROM accounts a \
         JOIN account_members m ON m.account_id = a.id \
         JOIN users u ON u.id = m.user_id \
         WHERE a.id = $1 LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to query account for Stripe customer");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let name: String = row.get("name");
    let email: String = row.get("email");

    // Create Stripe customer
    let mut params = CreateCustomer::new();
    params.name = Some(&name);
    params.email = Some(&email);
    params.metadata = Some(
        [("scrapix_account_id".to_string(), account_id.to_string())]
            .into_iter()
            .collect(),
    );

    let customer = Customer::create(stripe, params).await.map_err(|e| {
        error!(error = %e, "Failed to create Stripe customer");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create Stripe customer",
            "stripe_error",
        )
    })?;

    // Store the customer ID
    sqlx::query("UPDATE accounts SET stripe_customer_id = $1 WHERE id = $2")
        .bind(customer.id.as_str())
        .bind(account_id)
        .execute(pool)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to store stripe_customer_id");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?;

    info!(account_id = %account_id, customer_id = %customer.id, "Created Stripe customer");

    Ok(customer.id)
}

/// Get the user's account_id (re-exported from auth for convenience).
async fn get_account_id(pool: &sqlx::PgPool, user_id: uuid::Uuid) -> Result<uuid::Uuid, ApiError> {
    crate::auth::get_user_account_id(pool, user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /account/billing/setup-intent
///
/// Create a SetupIntent for the frontend to collect a payment method
/// (card details) via Stripe Elements. Returns a client_secret.
async fn create_setup_intent(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(stripe_state): Extension<StripeState>,
) -> Result<Json<SetupIntentResponse>, ApiError> {
    let account_id = get_account_id(&state.pool, user.user_id).await?;
    let customer_id = get_or_create_customer(&stripe_state.client, &state.pool, account_id).await?;

    let mut params = CreateSetupIntent::new();
    params.customer = Some(customer_id);
    params.payment_method_types = Some(vec!["card".to_string()]);
    params.metadata = Some(
        [("scrapix_account_id".to_string(), account_id.to_string())]
            .into_iter()
            .collect(),
    );

    let setup_intent = SetupIntent::create(&stripe_state.client, params)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create SetupIntent");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create setup intent",
                "stripe_error",
            )
        })?;

    Ok(Json(SetupIntentResponse {
        client_secret: setup_intent.client_secret.ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing client_secret",
                "stripe_error",
            )
        })?,
    }))
}

/// GET /account/billing/payment-methods
///
/// List all saved payment methods for the account's Stripe customer.
async fn list_payment_methods(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(stripe_state): Extension<StripeState>,
) -> Result<Json<Vec<PaymentMethodResponse>>, ApiError> {
    let account_id = get_account_id(&state.pool, user.user_id).await?;

    // Get stripe customer id — if none, return empty list
    let customer_id: Option<String> =
        sqlx::query_scalar("SELECT stripe_customer_id FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                error!(error = %e, "DB error");
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error",
                    "internal_error",
                )
            })?
            .flatten();

    let customer_id = match customer_id {
        Some(c) => c,
        None => return Ok(Json(vec![])),
    };

    let cid: CustomerId = customer_id.parse().map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Invalid stripe customer ID",
            "internal_error",
        )
    })?;

    // Get default payment method from our DB
    let default_pm: Option<String> =
        sqlx::query_scalar("SELECT stripe_default_payment_method_id FROM accounts WHERE id = $1")
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

    let mut params = ListPaymentMethods::new();
    params.customer = Some(cid);
    params.type_ = Some(PaymentMethodTypeFilter::Card);

    let methods = PaymentMethod::list(&stripe_state.client, &params)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to list payment methods");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to list payment methods",
                "stripe_error",
            )
        })?;

    let result: Vec<PaymentMethodResponse> = methods
        .data
        .iter()
        .map(|pm| {
            let card = pm.card.as_ref();
            PaymentMethodResponse {
                id: pm.id.to_string(),
                brand: card.map(|c| format!("{:?}", c.brand).to_lowercase()),
                last4: card.map(|c| c.last4.clone()),
                exp_month: card.map(|c| c.exp_month as i32),
                exp_year: card.map(|c| c.exp_year as i32),
                is_default: default_pm.as_deref() == Some(pm.id.as_str()),
            }
        })
        .collect();

    Ok(Json(result))
}

/// DELETE /account/billing/payment-methods/{id}
///
/// Detach a payment method from the customer.
async fn delete_payment_method(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(stripe_state): Extension<StripeState>,
    Path(pm_id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_account_id(&state.pool, user.user_id).await?;

    // Verify the payment method belongs to this account's customer
    let customer_id: Option<String> =
        sqlx::query_scalar("SELECT stripe_customer_id FROM accounts WHERE id = $1")
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

    let customer_id = customer_id
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "No Stripe customer", "no_customer"))?;

    let pm_id: PaymentMethodId = pm_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid payment method ID",
            "validation_error",
        )
    })?;

    // Fetch the payment method to verify ownership
    let pm = PaymentMethod::retrieve(&stripe_state.client, &pm_id, &[])
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to retrieve payment method");
            err(
                StatusCode::NOT_FOUND,
                "Payment method not found",
                "not_found",
            )
        })?;

    // Verify it belongs to this customer
    if pm.customer.as_ref().map(|c| c.id().to_string()) != Some(customer_id) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "Payment method does not belong to this account",
            "forbidden",
        ));
    }

    PaymentMethod::detach(&stripe_state.client, &pm.id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to detach payment method");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to remove payment method",
                "stripe_error",
            )
        })?;

    // If this was the default, clear it
    let default_pm: Option<String> =
        sqlx::query_scalar("SELECT stripe_default_payment_method_id FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .flatten();

    if default_pm.as_deref() == Some(pm.id.as_str()) {
        sqlx::query("UPDATE accounts SET stripe_default_payment_method_id = NULL WHERE id = $1")
            .bind(account_id)
            .execute(&state.pool)
            .await
            .ok();
    }

    info!(account_id = %account_id, payment_method = %pm.id, "Payment method detached");

    Ok(Json(MessageResponse {
        message: "Payment method removed".to_string(),
    }))
}

/// PATCH /account/billing/default-payment-method
///
/// Set the default payment method for the account.
async fn set_default_payment_method(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<SetDefaultPaymentMethodRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_account_id(&state.pool, user.user_id).await?;

    sqlx::query("UPDATE accounts SET stripe_default_payment_method_id = $1 WHERE id = $2")
        .bind(&req.payment_method_id)
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
        message: "Default payment method updated".to_string(),
    }))
}

/// POST /account/billing/purchase
///
/// Purchase a credit pack. Creates a PaymentIntent and charges the saved
/// payment method. If 3D Secure is required, returns `requires_action` with
/// a `client_secret` for the frontend to handle.
async fn purchase_credits(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(stripe_state): Extension<StripeState>,
    Json(req): Json<PurchaseCreditsRequest>,
) -> Result<Json<PurchaseResponse>, ApiError> {
    let amount_cents = price_for_credits(req.credits).ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid credit pack. Valid options: 1000, 5000, 10000, 50000",
            "validation_error",
        )
    })?;

    let account_id = get_account_id(&state.pool, user.user_id).await?;
    let customer_id = get_or_create_customer(&stripe_state.client, &state.pool, account_id).await?;

    // Determine payment method: explicit or default
    let pm_id = match req.payment_method_id {
        Some(ref id) => id.clone(),
        None => {
            let default: Option<String> = sqlx::query_scalar(
                "SELECT stripe_default_payment_method_id FROM accounts WHERE id = $1",
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
            .flatten();

            default.ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "No payment method on file. Please add a card first.",
                    "no_payment_method",
                )
            })?
        }
    };

    let pm_id: PaymentMethodId = pm_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid payment method ID",
            "validation_error",
        )
    })?;

    // Create PaymentIntent
    let mut params = CreatePaymentIntent::new(amount_cents, Currency::USD);
    params.customer = Some(customer_id);
    params.payment_method = Some(pm_id);
    params.confirm = Some(true);
    params.off_session = Some(stripe::PaymentIntentOffSession::Exists(true));
    params.metadata = Some(
        [
            ("scrapix_account_id".to_string(), account_id.to_string()),
            ("credits".to_string(), req.credits.to_string()),
            ("type".to_string(), "credit_purchase".to_string()),
        ]
        .into_iter()
        .collect(),
    );
    let description = format!("Scrapix: {} credits", req.credits);
    params.description = Some(&description);

    let pi = PaymentIntent::create(&stripe_state.client, params)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create PaymentIntent");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Payment failed. Please try again or use a different card.",
                "stripe_error",
            )
        })?;

    match pi.status {
        PaymentIntentStatus::Succeeded => {
            // Payment succeeded immediately — credits will be added by webhook
            // but we also add them here for instant feedback
            add_credits_for_payment(
                &state.pool,
                account_id,
                req.credits,
                pi.id.as_ref(),
                "Credit purchase",
            )
            .await?;

            Ok(Json(PurchaseResponse {
                status: "succeeded".to_string(),
                client_secret: None,
                credits: req.credits,
                amount_cents,
                message: format!("{} credits added to your account", req.credits),
            }))
        }
        PaymentIntentStatus::RequiresAction => {
            // 3D Secure or other action needed — return client_secret to frontend
            Ok(Json(PurchaseResponse {
                status: "requires_action".to_string(),
                client_secret: pi.client_secret,
                credits: req.credits,
                amount_cents,
                message: "Additional authentication required".to_string(),
            }))
        }
        other => {
            warn!(status = ?other, pi_id = %pi.id, "Unexpected PaymentIntent status");
            Err(err(
                StatusCode::BAD_REQUEST,
                "Payment could not be processed",
                "payment_failed",
            ))
        }
    }
}

/// Add credits to an account after a successful payment.
/// Idempotent: checks if a transaction with this stripe_payment_intent_id already exists.
async fn add_credits_for_payment(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    credits: i64,
    payment_intent_id: &str,
    description: &str,
) -> Result<(), ApiError> {
    // Idempotency check: see if we already processed this payment
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM transactions WHERE account_id = $1 AND metadata->>'stripe_payment_intent_id' = $2)",
    )
    .bind(account_id)
    .bind(payment_intent_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed idempotency check");
        err(StatusCode::INTERNAL_SERVER_ERROR, "Database error", "internal_error")
    })?;

    if exists {
        info!(account_id = %account_id, pi = %payment_intent_id, "Payment already processed, skipping");
        return Ok(());
    }

    let mut tx = pool.begin().await.map_err(|e| {
        error!(error = %e, "Failed to begin transaction");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let new_balance: i64 = sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance + $1 WHERE id = $2 RETURNING credits_balance",
    )
    .bind(credits)
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to update credits balance");
        err(StatusCode::INTERNAL_SERVER_ERROR, "Database error", "internal_error")
    })?;

    let metadata = serde_json::json!({
        "stripe_payment_intent_id": payment_intent_id,
    });

    sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description, metadata) \
         VALUES ($1, 'manual_topup', $2, $3, $4, $5)",
    )
    .bind(account_id)
    .bind(credits)
    .bind(new_balance)
    .bind(description)
    .bind(metadata)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to insert transaction");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    tx.commit().await.map_err(|e| {
        error!(error = %e, "Failed to commit");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    info!(
        account_id = %account_id,
        credits,
        new_balance,
        pi = %payment_intent_id,
        "Credits added via Stripe payment"
    );

    Ok(())
}

// ============================================================================
// Webhook handler
// ============================================================================

/// POST /webhooks/stripe
///
/// Receives Stripe webhook events. No auth required (verified by signature).
async fn stripe_webhook(
    Extension(stripe_state): Extension<StripeState>,
    Extension(pool): Extension<sqlx::PgPool>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Missing stripe-signature header".to_string(),
        ))?;

    let webhook_secret = stripe_state.webhook_secret.as_deref().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Webhook secret not configured".to_string(),
    ))?;

    let payload = std::str::from_utf8(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Invalid payload encoding".to_string(),
        )
    })?;

    let event = Webhook::construct_event(payload, signature, webhook_secret).map_err(|e| {
        warn!(error = %e, "Webhook signature verification failed");
        (
            StatusCode::BAD_REQUEST,
            "Webhook signature verification failed".to_string(),
        )
    })?;

    match event.type_ {
        EventType::PaymentIntentSucceeded => {
            if let EventObject::PaymentIntent(pi) = event.data.object {
                handle_payment_intent_succeeded(&pool, &pi).await;
            }
        }
        EventType::PaymentIntentPaymentFailed => {
            if let EventObject::PaymentIntent(pi) = event.data.object {
                warn!(
                    pi_id = %pi.id,
                    "Payment failed for PaymentIntent"
                );
            }
        }
        EventType::SetupIntentSucceeded => {
            if let EventObject::SetupIntent(si) = event.data.object {
                handle_setup_intent_succeeded(&pool, &si).await;
            }
        }
        _ => {
            // Ignore events we don't handle
        }
    }

    Ok(StatusCode::OK)
}

async fn handle_payment_intent_succeeded(pool: &sqlx::PgPool, pi: &PaymentIntent) {
    let metadata = &pi.metadata;

    let account_id_str = match metadata.get("scrapix_account_id") {
        Some(id) => id.clone(),
        None => {
            warn!(pi_id = %pi.id, "PaymentIntent missing scrapix_account_id metadata");
            return;
        }
    };

    let credits_str = match metadata.get("credits") {
        Some(c) => c.clone(),
        None => {
            warn!(pi_id = %pi.id, "PaymentIntent missing credits metadata");
            return;
        }
    };

    let account_id: uuid::Uuid = match account_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            warn!(pi_id = %pi.id, "Invalid account_id in metadata");
            return;
        }
    };

    let credits: i64 = match credits_str.parse() {
        Ok(c) => c,
        Err(_) => {
            warn!(pi_id = %pi.id, "Invalid credits in metadata");
            return;
        }
    };

    if let Err(e) = add_credits_for_payment(
        pool,
        account_id,
        credits,
        pi.id.as_ref(),
        "Credit purchase (Stripe)",
    )
    .await
    {
        error!(error = ?e, pi_id = %pi.id, "Failed to add credits from webhook");
    }
}

async fn handle_setup_intent_succeeded(pool: &sqlx::PgPool, si: &SetupIntent) {
    // When a setup intent succeeds, set the payment method as default if the account
    // doesn't have one yet
    let metadata = match &si.metadata {
        Some(m) => m,
        None => return,
    };

    let account_id_str = match metadata.get("scrapix_account_id") {
        Some(id) => id.clone(),
        None => return,
    };

    let account_id: uuid::Uuid = match account_id_str.parse() {
        Ok(id) => id,
        Err(_) => return,
    };

    let pm_id = match &si.payment_method {
        Some(pm) => pm.id().to_string(),
        None => return,
    };

    // Set as default only if no default exists yet
    let result = sqlx::query(
        "UPDATE accounts SET stripe_default_payment_method_id = $1 \
         WHERE id = $2 AND stripe_default_payment_method_id IS NULL",
    )
    .bind(&pm_id)
    .bind(account_id)
    .execute(pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => {
            info!(account_id = %account_id, pm = %pm_id, "Set default payment method from SetupIntent");
        }
        Ok(_) => {} // already had a default
        Err(e) => {
            warn!(error = %e, "Failed to set default payment method from webhook");
        }
    }
}

// ============================================================================
// Auto-topup with Stripe
// ============================================================================

/// Charge the account's saved payment method for an auto-topup.
/// Called from `billing::maybe_auto_topup` when a real payment is needed.
pub async fn charge_auto_topup(
    stripe: &StripeClient,
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    credits: i64,
) -> Result<(), String> {
    // Get customer ID and default payment method
    let row = sqlx::query(
        "SELECT stripe_customer_id, stripe_default_payment_method_id FROM accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("DB error: {e}"))?
    .ok_or("Account not found")?;

    let customer_id: Option<String> = row.get("stripe_customer_id");
    let pm_id: Option<String> = row.get("stripe_default_payment_method_id");

    let customer_id = customer_id.ok_or("No Stripe customer")?;
    let pm_id = pm_id.ok_or("No default payment method for auto-topup")?;

    let cid: CustomerId = customer_id.parse().map_err(|_| "Invalid customer ID")?;
    let pm: PaymentMethodId = pm_id.parse().map_err(|_| "Invalid payment method ID")?;

    // Calculate price — for auto-topup, use the closest pack or pro-rate
    let amount_cents = price_for_credits(credits).unwrap_or_else(|| {
        // Pro-rate at $0.008 per credit (5K pack rate) for non-standard amounts
        ((credits as f64) * 0.8).ceil() as i64
    });

    let mut params = CreatePaymentIntent::new(amount_cents, Currency::USD);
    params.customer = Some(cid);
    params.payment_method = Some(pm);
    params.confirm = Some(true);
    params.off_session = Some(stripe::PaymentIntentOffSession::Exists(true));
    params.metadata = Some(
        [
            ("scrapix_account_id".to_string(), account_id.to_string()),
            ("credits".to_string(), credits.to_string()),
            ("type".to_string(), "auto_topup".to_string()),
        ]
        .into_iter()
        .collect(),
    );
    let description = format!("Scrapix auto-topup: {} credits", credits);
    params.description = Some(&description);

    let pi = PaymentIntent::create(stripe, params)
        .await
        .map_err(|e| format!("Stripe error: {e}"))?;

    if pi.status == PaymentIntentStatus::Succeeded {
        // Credits will be added by the webhook, but also add here for immediacy
        add_credits_for_payment(
            pool,
            account_id,
            credits,
            pi.id.as_ref(),
            "Auto top-up (Stripe)",
        )
        .await
        .map_err(|e| format!("Failed to add credits: {}", e.0))?;
    } else {
        return Err(format!("Auto-topup payment status: {:?}", pi.status));
    }

    Ok(())
}

// ============================================================================
// Router
// ============================================================================

/// Stripe-related routes that require session auth.
pub fn stripe_session_routes(state: Arc<AuthState>, stripe_state: StripeState) -> Router {
    Router::new()
        .route("/account/billing/setup-intent", post(create_setup_intent))
        .route(
            "/account/billing/payment-methods",
            get(list_payment_methods),
        )
        .route(
            "/account/billing/payment-methods/{id}",
            delete(delete_payment_method),
        )
        .route(
            "/account/billing/default-payment-method",
            axum::routing::patch(set_default_payment_method),
        )
        .route("/account/billing/purchase", post(purchase_credits))
        .layer(Extension(stripe_state))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::validate_session,
        ))
        .with_state(state)
}

/// Stripe webhook route (no auth — verified by Stripe signature).
pub fn stripe_webhook_route(pool: sqlx::PgPool, stripe_state: StripeState) -> Router {
    Router::new()
        .route("/webhooks/stripe", post(stripe_webhook))
        .layer(Extension(stripe_state))
        .layer(Extension(pool))
}
