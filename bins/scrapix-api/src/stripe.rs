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
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::Row;
use std::sync::Arc;
use stripe::{
    Client as StripeClient, CreateCustomer, CreateInvoice, CreateInvoiceItem, CreateSetupIntent,
    Currency, Customer, CustomerId, Invoice, InvoicePendingInvoiceItemsBehavior, InvoiceStatus,
    ListInvoices, ListPaymentMethods, PaymentIntentStatus, PaymentMethod, PaymentMethodId,
    PaymentMethodTypeFilter, SetupIntent,
};
use tracing::{error, info, warn};

use crate::auth::{AuthState, AuthenticatedUser};
use crate::email::EmailClient;

// ============================================================================
// Volume-based tiered pricing
// ============================================================================

// Re-export pricing from the billing crate.
pub use scrapix_billing::calculate_price_cents;

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

    // The invoice may be paid without a PaymentIntent — e.g. when a customer
    // balance, credit note, or applied coupon covers the entire amount due,
    // Stripe marks the invoice as paid and skips PaymentIntent creation.
    // Check invoice.status first; only fall back to PaymentIntent status when
    // the invoice still requires an explicit charge (SCA flow, decline, etc).
    if invoice.status == Some(InvoiceStatus::Paid) {
        let pi_id = invoice
            .payment_intent
            .as_ref()
            .map(|pi| pi.id().to_string())
            .unwrap_or_else(|| invoice.id.to_string());

        add_credits_for_payment(
            &state.pool,
            account_id,
            req.credits,
            &pi_id,
            "Credit purchase",
        )
        .await?;

        return Ok(Json(PurchaseResponse {
            status: "succeeded".to_string(),
            client_secret: None,
            credits: req.credits,
            amount_cents,
            message: format!("{} credits added to your account", req.credits),
        }));
    }

    let pi_status = invoice
        .payment_intent
        .as_ref()
        .and_then(|pi| pi.as_object())
        .map(|pi| pi.status);

    match pi_status {
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
            warn!(
                pi_status = ?other,
                invoice_status = ?invoice.status,
                invoice_id = %invoice.id,
                "Unexpected payment status on invoice"
            );
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

    // 4. Pay the invoice — expands the payment_intent so we can check its status.
    //
    // Stripe gotcha: when collection_method = charge_automatically (default) and
    // a default_payment_method is set on the invoice, /finalize triggers an
    // immediate charge attempt. By the time our explicit /pay call lands, Stripe
    // responds 400 "Invoice is already paid". That's the expected path — treat
    // it as success and re-fetch the invoice (with expanded payment_intent) to
    // continue the ledger-crediting flow.
    let pay_params: std::collections::HashMap<&str, &str> =
        [("expand[]", "payment_intent")].into_iter().collect();
    let invoice: Invoice = match stripe
        .post_form(&format!("/invoices/{}/pay", invoice.id), pay_params)
        .await
    {
        Ok(inv) => inv,
        Err(e) if e.to_string().contains("Invoice is already paid") => {
            warn!(
                invoice_id = %invoice.id,
                "Invoice already paid by finalize; re-fetching to continue"
            );
            Invoice::retrieve(stripe, &invoice.id, &["payment_intent"])
                .await
                .map_err(|fetch_err| {
                    error!(
                        error = %fetch_err,
                        invoice_id = %invoice.id,
                        "Failed to re-fetch already-paid invoice"
                    );
                    err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to confirm invoice payment",
                        "stripe_error",
                    )
                })?
        }
        Err(e) => {
            error!(error = %e, invoice_id = %invoice.id, "Failed to pay invoice");
            return Err(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Payment failed. Please try again or use a different card.",
                "stripe_error",
            ));
        }
    };

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
/// Delegates to `scrapix_billing::add_credits_for_payment`.
async fn add_credits_for_payment(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    credits: i64,
    payment_intent_id: &str,
    description: &str,
) -> Result<(), ApiError> {
    scrapix_billing::add_credits_for_payment(
        pool,
        account_id,
        credits,
        payment_intent_id,
        description,
    )
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to add credits for payment");
        err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string(), e.code())
    })
}

// ============================================================================
// Webhook handler
// ============================================================================

/// Verify a Stripe webhook signature against the raw payload.
///
/// We can't use `stripe::Webhook::construct_event` because it deserializes the
/// payload into the library's typed `Event`/`EventObject`. Stripe regularly
/// adds new enum variants and fields, and async-stripe 0.38 fails to parse
/// payloads it doesn't recognize (returning `BadParse`), even when the
/// signature is valid. Doing HMAC verification ourselves and keeping the body
/// as JSON makes the handler resilient to Stripe API schema changes.
fn verify_stripe_signature(
    payload: &str,
    signature_header: &str,
    secret: &str,
) -> Result<(), String> {
    let mut timestamp: Option<i64> = None;
    let mut v1_sig: Option<&str> = None;
    for kv in signature_header.split(',') {
        if let Some((key, value)) = kv.split_once('=') {
            match key {
                "t" => timestamp = value.parse().ok(),
                "v1" => v1_sig = Some(value),
                _ => {}
            }
        }
    }
    let timestamp = timestamp.ok_or_else(|| "missing timestamp".to_string())?;
    let v1_sig = v1_sig.ok_or_else(|| "missing v1 signature".to_string())?;

    // Reject signatures older than 5 minutes to defend against replay.
    let now = chrono::Utc::now().timestamp();
    if (now - timestamp).abs() > 300 {
        return Err(format!(
            "timestamp out of tolerance (delta {}s)",
            (now - timestamp).abs()
        ));
    }

    let signed_payload = format!("{}.{}", timestamp, payload);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC key error: {}", e))?;
    mac.update(signed_payload.as_bytes());
    let expected = hex::decode(v1_sig).map_err(|e| format!("invalid v1 hex: {}", e))?;
    mac.verify_slice(&expected)
        .map_err(|_| "signature mismatch".to_string())
}

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

    if let Err(e) = verify_stripe_signature(payload, signature, webhook_secret) {
        warn!(error = %e, "Webhook signature verification failed");
        return Err((
            StatusCode::BAD_REQUEST,
            "Webhook signature verification failed".to_string(),
        ));
    }

    let event: serde_json::Value = serde_json::from_str(payload).map_err(|e| {
        warn!(error = %e, "Failed to parse webhook payload as JSON");
        (StatusCode::BAD_REQUEST, "Invalid JSON payload".to_string())
    })?;

    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let object = match event.pointer("/data/object") {
        Some(o) => o,
        None => {
            warn!(event_type, "Webhook event missing data.object");
            return Ok(StatusCode::OK);
        }
    };

    match event_type {
        "invoice.paid" => {
            handle_invoice_paid(&pool, object, email_client.as_ref()).await;
        }
        "payment_intent.succeeded" => {
            handle_payment_intent_succeeded(&pool, object, email_client.as_ref()).await;
        }
        "payment_intent.payment_failed" => {
            let pi_id = object.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            warn!(pi_id, "Payment failed for PaymentIntent");
        }
        "setup_intent.succeeded" => {
            handle_setup_intent_succeeded(&pool, object).await;
        }
        _ => {
            // Ignore events we don't handle
        }
    }

    Ok(StatusCode::OK)
}

/// Extract `data.object.metadata.<key>` as a string from a webhook payload.
fn metadata_str<'a>(obj: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    obj.get("metadata")
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_str())
}

/// Read `data.object.payment_intent` — Stripe sends it as either an ID string
/// or an expanded object. Returns the PaymentIntent ID either way.
fn extract_payment_intent_id(obj: &serde_json::Value) -> Option<String> {
    let pi = obj.get("payment_intent")?;
    if let Some(s) = pi.as_str() {
        return Some(s.to_string());
    }
    pi.get("id").and_then(|v| v.as_str()).map(String::from)
}

async fn handle_invoice_paid(
    pool: &sqlx::PgPool,
    inv: &serde_json::Value,
    email_client: Option<&EmailClient>,
) {
    let invoice_id = inv.get("id").and_then(|v| v.as_str()).unwrap_or("?");

    let account_id_str = match metadata_str(inv, "scrapix_account_id") {
        Some(s) => s,
        // Not a Scrapix invoice — ignore.
        None => return,
    };

    let credits_str = match metadata_str(inv, "credits") {
        Some(s) => s,
        None => {
            warn!(invoice_id, "Invoice missing credits metadata");
            return;
        }
    };

    let account_id: uuid::Uuid = match account_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            warn!(invoice_id, "Invalid account_id in invoice metadata");
            return;
        }
    };

    let credits: i64 = match credits_str.parse() {
        Ok(c) => c,
        Err(_) => {
            warn!(invoice_id, "Invalid credits in invoice metadata");
            return;
        }
    };

    // Idempotency key: prefer the PaymentIntent ID, fall back to invoice ID
    // (matches `purchase_credits` so sync + webhook can't double-credit).
    let pi_id = extract_payment_intent_id(inv).unwrap_or_else(|| invoice_id.to_string());

    if let Err(e) = add_credits_for_payment(
        pool,
        account_id,
        credits,
        &pi_id,
        "Credit purchase (Invoice)",
    )
    .await
    {
        error!(error = ?e, invoice_id, "Failed to add credits from invoice webhook");
        return;
    }

    // Send payment receipt via the reliable queue
    if let Some(mailer) = email_client {
        let amount_cents = inv.get("amount_paid").and_then(|v| v.as_i64()).unwrap_or(0);
        if let Some(email) = crate::email::get_account_email(pool, account_id).await {
            mailer
                .queue_payment_receipt(pool, &email, credits, amount_cents)
                .await;
        }
    }
}

async fn handle_payment_intent_succeeded(
    pool: &sqlx::PgPool,
    pi: &serde_json::Value,
    email_client: Option<&EmailClient>,
) {
    let pi_id = pi.get("id").and_then(|v| v.as_str()).unwrap_or("?");

    let account_id_str = match metadata_str(pi, "scrapix_account_id") {
        Some(s) => s,
        None => {
            warn!(pi_id, "PaymentIntent missing scrapix_account_id metadata");
            return;
        }
    };

    let credits_str = match metadata_str(pi, "credits") {
        Some(s) => s,
        None => {
            warn!(pi_id, "PaymentIntent missing credits metadata");
            return;
        }
    };

    let account_id: uuid::Uuid = match account_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            warn!(pi_id, "Invalid account_id in metadata");
            return;
        }
    };

    let credits: i64 = match credits_str.parse() {
        Ok(c) => c,
        Err(_) => {
            warn!(pi_id, "Invalid credits in metadata");
            return;
        }
    };

    if let Err(e) =
        add_credits_for_payment(pool, account_id, credits, pi_id, "Credit purchase (Stripe)").await
    {
        error!(error = ?e, pi_id, "Failed to add credits from webhook");
        return;
    }

    // Send payment receipt via the reliable queue
    if let Some(mailer) = email_client {
        let amount_cents = pi.get("amount").and_then(|v| v.as_i64()).unwrap_or(0);
        if let Some(email) = crate::email::get_account_email(pool, account_id).await {
            mailer
                .queue_payment_receipt(pool, &email, credits, amount_cents)
                .await;
        }
    }
}

async fn handle_setup_intent_succeeded(pool: &sqlx::PgPool, si: &serde_json::Value) {
    // When a setup intent succeeds, set the payment method as default if the
    // account doesn't have one yet.
    let account_id_str = match metadata_str(si, "scrapix_account_id") {
        Some(s) => s,
        None => return,
    };

    let account_id: uuid::Uuid = match account_id_str.parse() {
        Ok(id) => id,
        Err(_) => return,
    };

    // `payment_method` is either an ID string or an expanded object.
    let pm = match si.get("payment_method") {
        Some(v) => v,
        None => return,
    };
    let pm_id = if let Some(s) = pm.as_str() {
        s.to_string()
    } else if let Some(id) = pm.get("id").and_then(|v| v.as_str()) {
        id.to_string()
    } else {
        return;
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
) -> Result<String, String> {
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

    // Accept either a paid invoice (covered by customer balance / credit notes)
    // or a succeeded PaymentIntent. See `purchase_credits` for the same logic.
    if invoice.status == Some(InvoiceStatus::Paid) {
        let pi_id = invoice
            .payment_intent
            .as_ref()
            .map(|pi| pi.id().to_string())
            .unwrap_or_else(|| invoice.id.to_string());

        add_credits_for_payment(pool, account_id, credits, &pi_id, "Auto top-up (Stripe)")
            .await
            .map_err(|e| format!("Failed to add credits: {}", e.0))?;

        return Ok(pi_id);
    }

    let pi_status = invoice
        .payment_intent
        .as_ref()
        .and_then(|pi| pi.as_object())
        .map(|pi| pi.status);

    Err(format!(
        "Auto-topup payment status: pi={:?} invoice={:?}",
        pi_status, invoice.status
    ))
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
