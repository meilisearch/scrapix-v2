//! Hyperline webhook receiver.
//!
//! The request-path side of the migration — signed incoming events from
//! Hyperline (wallet credited/debited, invoice settled/errored, etc.) land
//! here. Verification and payload parsing live in
//! `scrapix_billing_hyperline::webhooks`; this module is responsible for the
//! axum handler, replay dedup via `hyperline_webhook_log`, and dispatch to
//! the local ledger / email pipeline.
//!
//! The event-dispatch match is intentionally minimal for the first cut —
//! every known event type is logged, and a focused follow-up PR will wire
//! `wallet.credited` into `scrapix_billing::ledger` once the ledger's
//! idempotency key is widened beyond `stripe_payment_intent_id`.

use axum::{
    body::Bytes,
    extract::Extension,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use scrapix_billing_hyperline::webhooks::{verify_signature, WebhookEnvelope, WebhookHeaders};
use tracing::{info, warn};

use crate::email::{get_account_email, EmailClient};

// ============================================================================
// State
// ============================================================================

/// Shared state for the Hyperline webhook route.
///
/// We keep this narrow — no full `HyperlineClient`, since webhook handling
/// doesn't call the control-plane API. The secret is stored here rather
/// than re-read from env on each request. The email client is optional
/// because the webhook route should still mount even when Resend is
/// unconfigured (dev / self-hosted) — low-balance emails just get
/// skipped with a warning.
#[derive(Clone)]
pub struct HyperlineWebhookState {
    pub webhook_secret: String,
    pub email_client: Option<EmailClient>,
}

impl HyperlineWebhookState {
    pub fn new(webhook_secret: String, email_client: Option<EmailClient>) -> Self {
        Self {
            webhook_secret,
            email_client,
        }
    }
}

// ============================================================================
// Handler
// ============================================================================

/// `POST /webhooks/hyperline` — receive a signed Hyperline event.
///
/// No auth header required: Hyperline signs the body with HMAC-SHA256 over
/// `id.timestamp.body`, and we reject anything that doesn't verify. A 5-min
/// timestamp-skew tolerance is enforced by [`verify_signature`].
///
/// Replay protection: `hyperline_webhook_log` has `webhook_id` as primary
/// key and we `INSERT … ON CONFLICT DO NOTHING`. If the insert affected
/// zero rows, we've seen this delivery before — return 200 without
/// dispatching so Hyperline stops retrying.
async fn hyperline_webhook(
    Extension(state): Extension<HyperlineWebhookState>,
    Extension(pool): Extension<sqlx::PgPool>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    // 1. Pull the three Svix-style headers.
    let id = header_str(&headers, "webhook-id")?;
    let timestamp = header_str(&headers, "webhook-timestamp")?;
    let signature = header_str(&headers, "webhook-signature")?;

    // 2. Verify HMAC + timestamp skew.
    let now = chrono::Utc::now().timestamp();
    verify_signature(
        &state.webhook_secret,
        WebhookHeaders {
            id,
            timestamp,
            signature,
        },
        &body,
        now,
    )
    .map_err(|e| {
        warn!(webhook_id = %id, error = %e, "hyperline webhook verification failed");
        (StatusCode::BAD_REQUEST, e.to_string())
    })?;

    // 3. Parse the envelope before we persist. We still write to the log
    //    even on parse failure so ops can inspect the body, but we return
    //    400 so Hyperline retries (in case this was transient malformed).
    let envelope: WebhookEnvelope = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            warn!(webhook_id = %id, error = %e, "hyperline webhook body is not a valid envelope");
            log_raw(&pool, id, &body, Some(&format!("parse error: {e}"))).await;
            return Err((StatusCode::BAD_REQUEST, format!("invalid body: {e}")));
        }
    };

    // 4. Dedupe insert. `INSERT … ON CONFLICT DO NOTHING` returns 0 rows
    //    when we've already processed this `webhook_id`.
    let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let inserted = sqlx::query(
        "INSERT INTO hyperline_webhook_log (webhook_id, event_type, body) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (webhook_id) DO NOTHING",
    )
    .bind(id)
    .bind(&envelope.event_type)
    .bind(&body_json)
    .execute(&pool)
    .await
    .map_err(|e| {
        warn!(webhook_id = %id, error = %e, "failed to insert hyperline_webhook_log row");
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    if inserted.rows_affected() == 0 {
        info!(
            webhook_id = %id,
            event_type = %envelope.event_type,
            "hyperline webhook already processed — skipping dispatch"
        );
        return Ok(StatusCode::OK);
    }

    // 5. Dispatch. Unknown events get logged and acked to stop retries.
    let dispatch_result = dispatch_event(&pool, &state, &envelope).await;

    // 6. Mark processed (success or error) on the log row so ops can tell
    //    "we saw it and tried" from "we saw it and crashed".
    let (processed_at_set, err_message) = match &dispatch_result {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.clone())),
    };
    if let Err(e) = sqlx::query(
        "UPDATE hyperline_webhook_log \
         SET processed_at = CASE WHEN $2 THEN now() ELSE processed_at END, \
             process_error = $3 \
         WHERE webhook_id = $1",
    )
    .bind(id)
    .bind(processed_at_set)
    .bind(err_message.as_deref())
    .execute(&pool)
    .await
    {
        warn!(webhook_id = %id, error = %e, "failed to update hyperline_webhook_log after dispatch");
    }

    // Always return 200 for a verified delivery — Hyperline retries on
    // non-2xx, and retrying won't fix a bug in our dispatch code.
    Ok(StatusCode::OK)
}

async fn dispatch_event(
    pool: &sqlx::PgPool,
    state: &HyperlineWebhookState,
    envelope: &WebhookEnvelope,
) -> Result<(), String> {
    match envelope.event_type.as_str() {
        "wallet.credited" | "credit.topup_transaction_created" => {
            // Resolve account + idempotency key from the envelope. Values
            // are extracted defensively — if anything is missing we log a
            // structured warning, ack the delivery (Hyperline retries are
            // pointless for schema issues), and return Ok.
            let id = envelope
                .data
                .get("object")
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str());
            let customer_id = envelope
                .data
                .get("object")
                .and_then(|o| o.get("customer_id"))
                .and_then(|v| v.as_str());
            // Credits unit is read from a custom `credits` property we
            // plan to ship on the Hyperline product-aggregator side. Real
            // amount→credits conversion (currency-aware) ships with the
            // reconcile worker.
            let credits = envelope
                .data
                .get("object")
                .and_then(|o| o.get("credits"))
                .and_then(|v| v.as_i64());

            match (id, customer_id, credits) {
                (Some(event_id), Some(cust), Some(credits_amt)) if credits_amt > 0 => {
                    let maybe_account: Option<uuid::Uuid> = sqlx::query_scalar(
                        "SELECT id FROM accounts WHERE hyperline_customer_id = $1",
                    )
                    .bind(cust)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| format!("account lookup: {e}"))?;

                    let Some(account_id) = maybe_account else {
                        warn!(
                            event_type = %envelope.event_type,
                            customer_id = %cust,
                            event_id = %event_id,
                            "hyperline: wallet.credited for unknown customer — acking"
                        );
                        return Ok(());
                    };

                    scrapix_billing::add_credits_from_provider(
                        pool,
                        account_id,
                        credits_amt,
                        "hyperline",
                        event_id,
                        "wallet_credit",
                        "Hyperline wallet credit",
                    )
                    .await
                    .map_err(|e| format!("ledger credit failed: {e}"))?;

                    info!(
                        account_id = %account_id,
                        event_id = %event_id,
                        credits = credits_amt,
                        "hyperline: wallet credited → ledger"
                    );
                }
                _ => {
                    warn!(
                        event_type = %envelope.event_type,
                        has_id = id.is_some(),
                        has_customer = customer_id.is_some(),
                        has_credits = credits.is_some(),
                        "hyperline: wallet.credited missing fields — acking without credit"
                    );
                }
            }
        }
        "wallet.debited" => {
            // Informational only — we debit locally at request time; this
            // is Hyperline's post-hoc notification. A drift check against
            // the ledger lives in `scrapix_billing_hyperline::reconcile`.
            info!(event_type = %envelope.event_type, "hyperline: wallet debited (mirror of local debit)");
        }
        "wallet.low_projected_balance" => {
            // Resolve (account, projected_balance) from the envelope and
            // send a low-balance warning to the account owner. Same
            // defensive extraction as wallet.credited.
            let customer_id = envelope
                .data
                .get("object")
                .and_then(|o| o.get("customer_id"))
                .and_then(|v| v.as_str());
            // `projected_balance` is Hyperline's post-window projection.
            // We accept either int or float; floats are rounded down.
            let projected_balance = envelope
                .data
                .get("object")
                .and_then(|o| o.get("projected_balance"))
                .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)));

            match (state.email_client.as_ref(), customer_id, projected_balance) {
                (Some(mailer), Some(cust), Some(balance)) => {
                    let maybe_account: Option<uuid::Uuid> = sqlx::query_scalar(
                        "SELECT id FROM accounts WHERE hyperline_customer_id = $1",
                    )
                    .bind(cust)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| format!("account lookup: {e}"))?;

                    let Some(account_id) = maybe_account else {
                        warn!(
                            event_type = %envelope.event_type,
                            customer_id = %cust,
                            "hyperline: low balance for unknown customer — acking"
                        );
                        return Ok(());
                    };

                    if let Some(email) = get_account_email(pool, account_id).await {
                        mailer.send_low_balance_warning(&email, balance);
                        info!(
                            account_id = %account_id,
                            projected_balance = balance,
                            "hyperline: low balance email dispatched"
                        );
                    } else {
                        warn!(
                            account_id = %account_id,
                            "hyperline: low balance — no owner email on file"
                        );
                    }
                }
                _ => {
                    warn!(
                        event_type = %envelope.event_type,
                        has_email_client = state.email_client.is_some(),
                        has_customer = customer_id.is_some(),
                        has_balance = projected_balance.is_some(),
                        "hyperline: low balance missing fields — acking without email"
                    );
                }
            }
        }
        "invoice.settled" | "invoice.errored" => {
            // Record the invoice event as an informational row in the
            // transactions ledger with amount = 0 — the actual credit
            // change rides in a separate wallet.credited event. This
            // gives the console a unified activity feed ("paid invoice
            // #abc for $X" next to "+1000 credits").
            let event_id = envelope
                .data
                .get("object")
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str());
            let customer_id = envelope
                .data
                .get("object")
                .and_then(|o| o.get("customer_id"))
                .and_then(|v| v.as_str());
            // `amount` is in the processor's minor unit (cents). Stored
            // in `metadata.processor_amount` so the UI can render "$X"
            // without touching `transactions.amount` (which is a credit
            // delta, not a currency amount).
            let processor_amount = envelope
                .data
                .get("object")
                .and_then(|o| o.get("amount"))
                .and_then(|v| v.as_i64());
            let currency = envelope
                .data
                .get("object")
                .and_then(|o| o.get("currency"))
                .and_then(|v| v.as_str());

            let (Some(event_id), Some(cust)) = (event_id, customer_id) else {
                warn!(
                    event_type = %envelope.event_type,
                    has_id = event_id.is_some(),
                    has_customer = customer_id.is_some(),
                    "hyperline: invoice event missing id/customer — acking without write"
                );
                return Ok(());
            };

            // Idempotency: same (provider, event_id) means we already
            // logged this invoice event. Short-circuit.
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS( \
                   SELECT 1 FROM transactions \
                   WHERE metadata->>'provider' = $1 \
                     AND metadata->>'provider_event_id' = $2 \
                 )",
            )
            .bind("hyperline")
            .bind(event_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("invoice dedupe check: {e}"))?;
            if exists {
                info!(
                    event_type = %envelope.event_type,
                    event_id = %event_id,
                    "hyperline: invoice event already logged — skipping"
                );
                return Ok(());
            }

            let maybe_account: Option<(uuid::Uuid, i64)> = sqlx::query_as(
                "SELECT id, credits_balance FROM accounts WHERE hyperline_customer_id = $1",
            )
            .bind(cust)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("account lookup: {e}"))?;

            let Some((account_id, balance_after)) = maybe_account else {
                warn!(
                    event_type = %envelope.event_type,
                    customer_id = %cust,
                    "hyperline: invoice event for unknown customer — acking"
                );
                return Ok(());
            };

            let tx_type = if envelope.event_type == "invoice.settled" {
                "invoice_settled"
            } else {
                "invoice_errored"
            };
            let description = format!("Hyperline invoice {}", envelope.event_type);
            let metadata = serde_json::json!({
                "provider": "hyperline",
                "provider_event_id": event_id,
                "processor_amount": processor_amount,
                "currency": currency,
            });

            sqlx::query(
                "INSERT INTO transactions (account_id, type, amount, balance_after, description, metadata) \
                 VALUES ($1, $2, 0, $3, $4, $5)",
            )
            .bind(account_id)
            .bind(tx_type)
            .bind(balance_after)
            .bind(&description)
            .bind(metadata)
            .execute(pool)
            .await
            .map_err(|e| format!("insert invoice transaction: {e}"))?;

            info!(
                account_id = %account_id,
                event_id = %event_id,
                tx_type,
                "hyperline: invoice event recorded in transactions"
            );
        }
        "payment_method.errored" | "payment_method.expired" => {
            info!(event_type = %envelope.event_type, "hyperline: payment method issue (flag account is TODO)");
        }
        "subscription.cancelled" => {
            // Hard lock: flip `accounts.active = false`. Both the credit
            // ledger (`check_credits`) and the Postgres
            // `lookup_api_key_account` function gate on `active = true`, so
            // this simultaneously blocks new requests and rejects API key
            // auth. Re-activation is a manual operator action — we never
            // flip back to true from a webhook.
            let customer_id = envelope
                .data
                .get("object")
                .and_then(|o| o.get("customer_id"))
                .and_then(|v| v.as_str());

            let Some(cust) = customer_id else {
                warn!(
                    event_type = %envelope.event_type,
                    "hyperline: subscription.cancelled missing customer_id — acking without action"
                );
                return Ok(());
            };

            let affected = sqlx::query(
                "UPDATE accounts SET active = false \
                 WHERE hyperline_customer_id = $1 AND active = true",
            )
            .bind(cust)
            .execute(pool)
            .await
            .map_err(|e| format!("deactivate account: {e}"))?;

            if affected.rows_affected() == 0 {
                warn!(
                    event_type = %envelope.event_type,
                    customer_id = %cust,
                    "hyperline: subscription.cancelled for unknown or already-inactive customer"
                );
            } else {
                info!(
                    customer_id = %cust,
                    "hyperline: account deactivated on subscription.cancelled"
                );
            }
        }
        other => {
            info!(event_type = %other, "hyperline: unknown event type — acked and logged");
        }
    }
    Ok(())
}

async fn log_raw(pool: &sqlx::PgPool, id: &str, body: &[u8], err: Option<&str>) {
    let body_json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    if let Err(e) = sqlx::query(
        "INSERT INTO hyperline_webhook_log (webhook_id, event_type, body, process_error) \
         VALUES ($1, 'unknown', $2, $3) \
         ON CONFLICT (webhook_id) DO NOTHING",
    )
    .bind(id)
    .bind(body_json)
    .bind(err)
    .execute(pool)
    .await
    {
        warn!(webhook_id = %id, error = %e, "failed to log unparseable hyperline webhook");
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, (StatusCode, String)> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("missing header: {name}")))
}

// ============================================================================
// Route
// ============================================================================

/// Build the `/webhooks/hyperline` router.
///
/// No auth middleware — verification is per-request against the HMAC secret
/// in [`HyperlineWebhookState`]. Wire into the main app in `lib.rs` via
/// `app = app.merge(hyperline::webhook_route(pool, secret))`.
pub fn webhook_route(
    pool: sqlx::PgPool,
    webhook_secret: String,
    email_client: Option<EmailClient>,
) -> Router {
    let state = HyperlineWebhookState::new(webhook_secret, email_client);
    Router::new()
        .route("/webhooks/hyperline", post(hyperline_webhook))
        .layer(Extension(state))
        .layer(Extension(pool))
}
