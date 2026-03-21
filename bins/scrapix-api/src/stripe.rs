//! Stripe payment integration.
//!
//! Handles customer creation, payment methods, credit purchases via Invoices,
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
    Client as StripeClient, CreateCustomer, CreateInvoice, CreateInvoiceItem, CreateSetupIntent,
    Currency, Customer, CustomerId, EventObject, EventType, Invoice,
    InvoicePendingInvoiceItemsBehavior, InvoiceStatus, ListInvoices, ListPaymentMethods,
    PaymentIntent, PaymentIntentStatus, PaymentMethod, PaymentMethodId, PaymentMethodTypeFilter,
    SetupIntent, Webhook,
};
use tracing::{error, info, warn};

use crate::auth::{AuthState, AuthenticatedUser};
use crate::email::EmailClient;

// ============================================================================
// Volume-based tiered pricing
// ============================================================================

/// Calculate the price in cents for a given number of credits.
/// Volume-based: the entire quantity is priced at the tier rate.
///
/// | Volume      | Per credit | Per 1K |
/// |-------------|-----------|--------|
/// | 1–999       | $0.010    | $10    |
/// | 1,000–4,999 | $0.008    | $8     |
/// | 5,000–9,999 | $0.007    | $7     |
/// | 10,000+     | $0.005    | $5     |
pub fn calculate_price_cents(credits: i64) -> i64 {
    // Unit price in tenths of a cent to avoid floating point
    let rate_tenths = if credits >= 10_000 {
        5 // $0.005 = 0.5 cents
    } else if credits >= 5_000 {
        7 // $0.007 = 0.7 cents
    } else if credits >= 1_000 {
        8 // $0.008 = 0.8 cents
    } else {
        10 // $0.010 = 1.0 cent
    };
    // cents = credits * rate_tenths / 10, ceiling
    (credits * rate_tenths + 9) / 10
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

#[derive(Serialize)]
pub(crate) struct InvoiceResponse {
    id: String,
    number: Option<String>,
    amount_cents: i64,
    credits: Option<i64>,
    status: String,
    description: Option<String>,
    created_at: String,
    invoice_pdf: Option<String>,
    hosted_invoice_url: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PricingTier {
    up_to: Option<i64>,
    unit_price_cents: f64,
    per_1k: f64,
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
async fn get_account_id(
    pool: &sqlx::PgPool,
    user: &AuthenticatedUser,
) -> Result<uuid::Uuid, ApiError> {
    crate::auth::get_user_account_id(pool, user.user_id, user.selected_account_id)
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
    let account_id = get_account_id(&state.pool, &user).await?;
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
    let account_id = get_account_id(&state.pool, &user).await?;

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
    let account_id = get_account_id(&state.pool, &user).await?;

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
    let account_id = get_account_id(&state.pool, &user).await?;

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
/// Purchase a credit pack. Creates a Stripe Invoice with line items, finalizes
/// and pays it. This generates a proper invoice with PDF. If 3D Secure is
/// required, returns `requires_action` with a `client_secret` for the frontend.
async fn purchase_credits(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(stripe_state): Extension<StripeState>,
    Json(req): Json<PurchaseCreditsRequest>,
) -> Result<Json<PurchaseResponse>, ApiError> {
    if req.credits < 100 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Minimum purchase is 100 credits",
            "validation_error",
        ));
    }

    let amount_cents = calculate_price_cents(req.credits);

    let account_id = get_account_id(&state.pool, &user).await?;
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

    // Create the invoice, pay it, and add credits
    let invoice = create_and_pay_invoice(
        &stripe_state.client,
        customer_id,
        account_id,
        &pm_id,
        req.credits,
        amount_cents,
        "credit_purchase",
    )
    .await?;

    // Check the payment intent status on the paid invoice
    let pi_status = invoice
        .payment_intent
        .as_ref()
        .and_then(|pi| pi.as_object())
        .map(|pi| pi.status);

    match pi_status {
        Some(PaymentIntentStatus::Succeeded) => {
            let pi_id = invoice
                .payment_intent
                .as_ref()
                .map(|pi| pi.id().to_string())
                .unwrap_or_default();

            add_credits_for_payment(
                &state.pool,
                account_id,
                req.credits,
                &pi_id,
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
        Some(PaymentIntentStatus::RequiresAction) => {
            let client_secret = invoice
                .payment_intent
                .as_ref()
                .and_then(|pi| pi.as_object())
                .and_then(|pi| pi.client_secret.clone());

            Ok(Json(PurchaseResponse {
                status: "requires_action".to_string(),
                client_secret,
                credits: req.credits,
                amount_cents,
                message: "Additional authentication required".to_string(),
            }))
        }
        other => {
            warn!(status = ?other, invoice_id = %invoice.id, "Unexpected payment status on invoice");
            Err(err(
                StatusCode::BAD_REQUEST,
                "Payment could not be processed",
                "payment_failed",
            ))
        }
    }
}

/// Create a Stripe Invoice with a line item, finalize it, and pay it.
/// Returns the paid Invoice object (with `invoice_pdf`, `hosted_invoice_url`, etc.).
async fn create_and_pay_invoice(
    stripe: &StripeClient,
    customer_id: CustomerId,
    account_id: uuid::Uuid,
    payment_method_id: &str,
    credits: i64,
    amount_cents: i64,
    purchase_type: &str,
) -> Result<Invoice, ApiError> {
    // 1. Create an invoice item (pending, attached to customer)
    let item_description = format!("Scrapix: {} credits", credits);
    let mut item_params = CreateInvoiceItem::new(customer_id.clone());
    item_params.amount = Some(amount_cents);
    item_params.currency = Some(Currency::USD);
    item_params.description = Some(&item_description);

    stripe::InvoiceItem::create(stripe, item_params)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create InvoiceItem");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create invoice item",
                "stripe_error",
            )
        })?;

    // 2. Create a draft invoice (picks up the pending invoice item)
    let description = format!("Scrapix: {} credits", credits);
    let mut invoice_params = CreateInvoice::new();
    invoice_params.customer = Some(customer_id);
    invoice_params.collection_method = Some(stripe::CollectionMethod::ChargeAutomatically);
    invoice_params.auto_advance = Some(false); // we'll finalize and pay manually
    invoice_params.default_payment_method = Some(payment_method_id);
    invoice_params.description = Some(&description);
    invoice_params.pending_invoice_items_behavior =
        Some(InvoicePendingInvoiceItemsBehavior::Include);
    invoice_params.metadata = Some(
        [
            ("scrapix_account_id".to_string(), account_id.to_string()),
            ("credits".to_string(), credits.to_string()),
            ("type".to_string(), purchase_type.to_string()),
        ]
        .into_iter()
        .collect(),
    );

    let invoice = Invoice::create(stripe, invoice_params).await.map_err(|e| {
        error!(error = %e, "Failed to create Invoice");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create invoice",
            "stripe_error",
        )
    })?;

    // 3. Finalize the invoice
    let finalize_params: std::collections::HashMap<&str, &str> =
        [("auto_advance", "false")].into_iter().collect();
    let invoice: Invoice = stripe
        .post_form(
            &format!("/invoices/{}/finalize", invoice.id),
            finalize_params,
        )
        .await
        .map_err(|e| {
            error!(error = %e, invoice_id = %invoice.id, "Failed to finalize invoice");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to finalize invoice",
                "stripe_error",
            )
        })?;

    // 4. Pay the invoice — expands the payment_intent so we can check its status
    let pay_params: std::collections::HashMap<&str, &str> =
        [("expand[]", "payment_intent")].into_iter().collect();
    let invoice: Invoice = stripe
        .post_form(&format!("/invoices/{}/pay", invoice.id), pay_params)
        .await
        .map_err(|e| {
            error!(error = %e, invoice_id = %invoice.id, "Failed to pay invoice");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Payment failed. Please try again or use a different card.",
                "stripe_error",
            )
        })?;

    info!(
        account_id = %account_id,
        invoice_id = %invoice.id,
        credits,
        amount_cents,
        "Invoice created and paid"
    );

    Ok(invoice)
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
    Extension(email_client): Extension<Option<EmailClient>>,
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
        EventType::InvoicePaid => {
            if let EventObject::Invoice(inv) = event.data.object {
                handle_invoice_paid(&pool, &inv, email_client.as_ref()).await;
            }
        }
        EventType::PaymentIntentSucceeded => {
            if let EventObject::PaymentIntent(pi) = event.data.object {
                handle_payment_intent_succeeded(&pool, &pi, email_client.as_ref()).await;
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

async fn handle_invoice_paid(
    pool: &sqlx::PgPool,
    inv: &Invoice,
    email_client: Option<&EmailClient>,
) {
    let metadata = match &inv.metadata {
        Some(m) => m,
        None => {
            warn!(invoice_id = %inv.id, "Invoice missing metadata");
            return;
        }
    };

    let account_id_str = match metadata.get("scrapix_account_id") {
        Some(id) => id.clone(),
        None => {
            // Not a Scrapix invoice — ignore
            return;
        }
    };

    let credits_str = match metadata.get("credits") {
        Some(c) => c.clone(),
        None => {
            warn!(invoice_id = %inv.id, "Invoice missing credits metadata");
            return;
        }
    };

    let account_id: uuid::Uuid = match account_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            warn!(invoice_id = %inv.id, "Invalid account_id in invoice metadata");
            return;
        }
    };

    let credits: i64 = match credits_str.parse() {
        Ok(c) => c,
        Err(_) => {
            warn!(invoice_id = %inv.id, "Invalid credits in invoice metadata");
            return;
        }
    };

    // Use the invoice's payment_intent ID for idempotency
    let pi_id = inv
        .payment_intent
        .as_ref()
        .map(|pi| pi.id().to_string())
        .unwrap_or_else(|| inv.id.to_string());

    if let Err(e) = add_credits_for_payment(
        pool,
        account_id,
        credits,
        &pi_id,
        "Credit purchase (Invoice)",
    )
    .await
    {
        error!(error = ?e, invoice_id = %inv.id, "Failed to add credits from invoice webhook");
        return;
    }

    // Send payment receipt email
    if let Some(mailer) = email_client {
        let amount_cents = inv.amount_paid.unwrap_or(0);
        if let Some(email) = crate::email::get_account_email(pool, account_id).await {
            mailer.send_payment_receipt(&email, credits, amount_cents);
        }
    }
}

async fn handle_payment_intent_succeeded(
    pool: &sqlx::PgPool,
    pi: &PaymentIntent,
    email_client: Option<&EmailClient>,
) {
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
        return;
    }

    // Send payment receipt email
    if let Some(mailer) = email_client {
        let amount_cents = pi.amount;
        if let Some(email) = crate::email::get_account_email(pool, account_id).await {
            mailer.send_payment_receipt(&email, credits, amount_cents);
        }
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

    let amount_cents = calculate_price_cents(credits);

    let invoice = create_and_pay_invoice(
        stripe,
        cid,
        account_id,
        &pm_id,
        credits,
        amount_cents,
        "auto_topup",
    )
    .await
    .map_err(|e| format!("Invoice error: {}", e.1.error))?;

    let pi_status = invoice
        .payment_intent
        .as_ref()
        .and_then(|pi| pi.as_object())
        .map(|pi| pi.status);

    if pi_status == Some(PaymentIntentStatus::Succeeded) {
        let pi_id = invoice
            .payment_intent
            .as_ref()
            .map(|pi| pi.id().to_string())
            .unwrap_or_default();

        add_credits_for_payment(pool, account_id, credits, &pi_id, "Auto top-up (Stripe)")
            .await
            .map_err(|e| format!("Failed to add credits: {}", e.0))?;
    } else {
        return Err(format!("Auto-topup payment status: {:?}", pi_status));
    }

    Ok(())
}

// ============================================================================
// Invoices
// ============================================================================

/// GET /account/billing/invoices
///
/// List actual Stripe Invoices for the account, with PDF download links.
async fn list_invoices(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(stripe_state): Extension<StripeState>,
) -> Result<Json<Vec<InvoiceResponse>>, ApiError> {
    let account_id = get_account_id(&state.pool, &user).await?;

    let customer_id: Option<String> =
        sqlx::query_scalar("SELECT stripe_customer_id FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                error!(error = %e, "DB error fetching stripe_customer_id");
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

    let mut params = ListInvoices::new();
    params.customer = Some(cid);
    params.status = Some(InvoiceStatus::Paid);
    params.limit = Some(50);

    let stripe_invoices = Invoice::list(&stripe_state.client, &params)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to list invoices");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to list invoices",
                "stripe_error",
            )
        })?;

    let invoices: Vec<InvoiceResponse> = stripe_invoices
        .data
        .iter()
        .map(|inv| {
            let credits = inv
                .metadata
                .as_ref()
                .and_then(|m| m.get("credits"))
                .and_then(|c| c.parse::<i64>().ok());

            let status = inv
                .status
                .map(|s| s.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            InvoiceResponse {
                id: inv.id.to_string(),
                number: inv.number.clone(),
                amount_cents: inv.amount_paid.unwrap_or(0),
                credits,
                status,
                description: inv.description.clone(),
                created_at: inv
                    .created
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                invoice_pdf: inv.invoice_pdf.clone(),
                hosted_invoice_url: inv.hosted_invoice_url.clone(),
            }
        })
        .collect();

    Ok(Json(invoices))
}

// ============================================================================
// Pricing
// ============================================================================

/// GET /account/billing/pricing
///
/// Returns the volume-based pricing tiers.
async fn get_pricing() -> Json<Vec<PricingTier>> {
    Json(vec![
        PricingTier {
            up_to: Some(999),
            unit_price_cents: 1.0,
            per_1k: 10.0,
        },
        PricingTier {
            up_to: Some(4_999),
            unit_price_cents: 0.8,
            per_1k: 8.0,
        },
        PricingTier {
            up_to: Some(9_999),
            unit_price_cents: 0.7,
            per_1k: 7.0,
        },
        PricingTier {
            up_to: None,
            unit_price_cents: 0.5,
            per_1k: 5.0,
        },
    ])
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
        .route("/account/billing/invoices", get(list_invoices))
        .route("/account/billing/pricing", get(get_pricing))
        .layer(Extension(stripe_state))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::validate_session,
        ))
        .with_state(state)
}

/// Stripe webhook route (no auth — verified by Stripe signature).
pub fn stripe_webhook_route(
    pool: sqlx::PgPool,
    stripe_state: StripeState,
    email_client: Option<EmailClient>,
) -> Router {
    Router::new()
        .route("/webhooks/stripe", post(stripe_webhook))
        .layer(Extension(stripe_state))
        .layer(Extension(pool))
        .layer(Extension(email_client))
}
