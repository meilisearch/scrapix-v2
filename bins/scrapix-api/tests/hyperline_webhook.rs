//! DB-bound integration tests for `POST /webhooks/hyperline`.
//!
//! These drive the real axum router in-process against a Postgres pool,
//! signing each payload with the same HMAC-SHA256 algorithm Hyperline
//! uses so the verifier sees real bytes end-to-end. We don't stub
//! `verify_signature` — that's the whole point of this suite.
//!
//! Gating: tests are `#[ignore]` by default and require
//! `TEST_DATABASE_URL` to point at a Postgres whose schema matches
//! `deploy/postgres/init.sql`. The simplest way to run them:
//!
//! ```sh
//! docker compose up -d postgres
//! TEST_DATABASE_URL=postgres://scrapix:scrapix@localhost:5433/scrapix \
//!   cargo test -p scrapix-api --test hyperline_webhook -- --ignored --nocapture
//! ```
//!
//! Tests don't clean up after themselves; they use fresh UUIDs for
//! every row so re-runs never collide. The dev DB accumulates harmless
//! rows; `docker compose down -v` resets it.
//!
//! What's covered:
//! - Signature verification rejects invalid signatures and stale
//!   timestamps (matches the unit tests in the verifier crate, but
//!   exercised through the axum extract layer — catches regressions in
//!   header parsing / extractor wiring).
//! - `wallet.credited` → ledger credit + `transactions` row.
//! - Webhook-id dedupe: same delivery replayed is a no-op.
//! - Ledger-level dedupe: new webhook_id but same `provider_event_id`
//!   credits only once.
//! - `invoice.settled` clears `payment_method_status`.
//! - `invoice.errored` records a transaction without clearing the flag.
//! - `payment_method.expired` sets `payment_method_status`.
//! - `subscription.cancelled` flips `accounts.active = false`.
//! - Unknown event type → 200, logged (so Hyperline stops retrying).

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt;
use scrapix_api::hyperline;
use sha2::Sha256;
use sqlx::{PgPool, Row};
use std::sync::Once;
use tower::ServiceExt;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// HMAC key bytes shared by every test in this file. We derive the
/// `whsec_` secret string from it so the handler and the test agree.
const WEBHOOK_KEY_RAW: &[u8] = b"scrapix-test-webhook-key-do-not-use-in-prod";

static INIT: Once = Once::new();

/// Connect to `TEST_DATABASE_URL` or return `None` (tests then skip).
///
/// We don't panic on a missing var — `#[ignore]` is doing the real
/// gating, but making the helper return Option keeps the "no DB
/// available" message readable.
async fn try_pool() -> Option<PgPool> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let pool = sqlx::PgPool::connect(&url)
        .await
        .expect("TEST_DATABASE_URL is set but the connection failed");

    // Run the schema once per process. init.sql is fully idempotent (IF
    // NOT EXISTS everywhere), so running it multiple times is safe — but
    // doing it once saves a few ms per test.
    INIT.call_once(|| {
        let pool = pool.clone();
        // Best-effort: if schema application fails the tests will surface
        // the real error downstream, no need to bubble it up here.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let sql = std::fs::read_to_string(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../deploy/postgres/init.sql"
                ))
                .expect("reading deploy/postgres/init.sql");
                // sqlx can only run one statement per `execute`, so we
                // walk the file. Splitting on `;\n` is coarse but good
                // enough for init.sql (no function bodies with embedded
                // semicolons, which is the usual gotcha).
                for stmt in sql.split(";\n") {
                    let trimmed = stmt.trim();
                    if trimmed.is_empty() || trimmed.starts_with("--") {
                        continue;
                    }
                    // Ignore failures here (e.g. re-running in a dirty
                    // DB): init.sql is entirely IF NOT EXISTS / IF NOT
                    // EXISTS wrapped, but manual DB surgery between
                    // runs can leave it in a state where a subset
                    // errors. Tests then fail loudly on real assertions.
                    let _ = sqlx::query(trimmed).execute(&pool).await;
                }
            });
        });
    });

    Some(pool)
}

/// Build the `whsec_`-prefixed secret string the handler expects.
fn webhook_secret() -> String {
    format!("whsec_{}", B64.encode(WEBHOOK_KEY_RAW))
}

/// Sign a body per Hyperline's Svix-style HMAC over `id.timestamp.body`.
fn sign(id: &str, ts: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(WEBHOOK_KEY_RAW).expect("hmac key");
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(ts.as_bytes());
    mac.update(b".");
    mac.update(body);
    let b64 = B64.encode(mac.finalize().into_bytes());
    format!("v1,{b64}")
}

/// Build the webhook router with a fresh state. No email client so the
/// low-balance branch is exercised without a real mailer (it logs and
/// moves on — the transactions/accounts side effects still land).
fn router(pool: PgPool) -> Router {
    hyperline::webhook_route(pool, webhook_secret(), None)
}

/// Seed a user → account pair. Returns `(account_id, hyperline_customer_id)`.
///
/// Each account gets a unique `hyperline_customer_id` so tests don't
/// collide on the UNIQUE constraint, and the returned handle is used
/// as the `customer_id` in webhook payloads.
async fn seed_account(pool: &PgPool) -> (Uuid, String) {
    let customer_id = format!("cus_test_{}", Uuid::new_v4().simple());
    let account_id: Uuid = sqlx::query_scalar(
        "INSERT INTO accounts (name, hyperline_customer_id, credits_balance) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(format!("test-account-{}", Uuid::new_v4().simple()))
    .bind(&customer_id)
    .bind(100_i64)
    .fetch_one(pool)
    .await
    .expect("seed_account");
    (account_id, customer_id)
}

/// Construct a signed `Request<Body>` for the webhook route.
fn signed_request(
    id: &str,
    timestamp: i64,
    body: serde_json::Value,
    signature_override: Option<&str>,
) -> Request<Body> {
    let ts_str = timestamp.to_string();
    let body_bytes = serde_json::to_vec(&body).expect("serialize body");
    let sig = signature_override
        .map(str::to_owned)
        .unwrap_or_else(|| sign(id, &ts_str, &body_bytes));
    Request::builder()
        .method("POST")
        .uri("/webhooks/hyperline")
        .header("content-type", "application/json")
        .header("webhook-id", id)
        .header("webhook-timestamp", &ts_str)
        .header("webhook-signature", &sig)
        .body(Body::from(body_bytes))
        .expect("build request")
}

/// Drive the router once and return the status code. Body is consumed
/// so tests can follow up with DB assertions.
async fn send(router: &Router, req: Request<Body>) -> StatusCode {
    let resp = router.clone().oneshot(req).await.expect("router oneshot");
    let status = resp.status();
    // Drain the body so the connection "finishes" cleanly.
    let _ = resp.into_body().collect().await;
    status
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn rejects_invalid_signature() {
    let Some(pool) = try_pool().await else { return };
    let app = router(pool);

    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        chrono::Utc::now().timestamp(),
        serde_json::json!({"type": "wallet.debited", "data": {}}),
        Some("v1,bogus-signature-bytes"),
    );
    assert_eq!(send(&app, req).await, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn rejects_stale_timestamp() {
    let Some(pool) = try_pool().await else { return };
    let app = router(pool);

    // 10 minutes in the past — well outside the 5-min skew window.
    let stale = chrono::Utc::now().timestamp() - 600;
    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        stale,
        serde_json::json!({"type": "wallet.debited", "data": {}}),
        None,
    );
    assert_eq!(send(&app, req).await, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn wallet_credited_adds_credits_and_records_transaction() {
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    let app = router(pool.clone());

    let event_id = format!("evt_{}", Uuid::new_v4().simple());
    let body = serde_json::json!({
        "type": "wallet.credited",
        "data": {
            "object": {
                "id": event_id,
                "customer_id": customer_id,
                "credits": 500,
            }
        }
    });
    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        chrono::Utc::now().timestamp(),
        body,
        None,
    );
    assert_eq!(send(&app, req).await, StatusCode::OK);

    // Balance bumped by 500 (from the seed's 100 → 600).
    let balance: i64 = sqlx::query_scalar("SELECT credits_balance FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(balance, 600);

    // One `wallet_credit` transaction row with the provider event id.
    let row = sqlx::query(
        "SELECT type, amount, metadata FROM transactions \
         WHERE account_id = $1 AND metadata->>'provider_event_id' = $2",
    )
    .bind(account_id)
    .bind(&event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("type"), "wallet_credit");
    assert_eq!(row.get::<i64, _>("amount"), 500);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn replay_same_webhook_id_is_no_op() {
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    let app = router(pool.clone());

    let webhook_id = format!("msg_{}", Uuid::new_v4().simple());
    let event_id = format!("evt_{}", Uuid::new_v4().simple());
    let body = serde_json::json!({
        "type": "wallet.credited",
        "data": {
            "object": {
                "id": event_id,
                "customer_id": customer_id,
                "credits": 250,
            }
        }
    });
    let ts = chrono::Utc::now().timestamp();

    // First delivery — accepted, credits applied.
    let req1 = signed_request(&webhook_id, ts, body.clone(), None);
    assert_eq!(send(&app, req1).await, StatusCode::OK);

    // Second delivery with the *same* webhook_id — dedupe should kick
    // in at the hyperline_webhook_log layer before dispatch runs.
    let req2 = signed_request(&webhook_id, ts, body, None);
    assert_eq!(send(&app, req2).await, StatusCode::OK);

    let balance: i64 = sqlx::query_scalar("SELECT credits_balance FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    // Still 350 (100 seed + 250 once), not 600.
    assert_eq!(balance, 350);

    let tx_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE account_id = $1 AND metadata->>'provider_event_id' = $2",
    )
    .bind(account_id)
    .bind(&event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tx_count, 1);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn replay_new_webhook_id_same_event_id_is_ledger_idempotent() {
    // Covers the dual idempotency model: if Hyperline re-broadcasts the
    // same business event under a new delivery id (which bypasses the
    // webhook_log dedup), the ledger's (provider, provider_event_id)
    // check keeps us honest.
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    let app = router(pool.clone());

    let event_id = format!("evt_{}", Uuid::new_v4().simple());
    let body = serde_json::json!({
        "type": "wallet.credited",
        "data": {
            "object": {
                "id": event_id,
                "customer_id": customer_id,
                "credits": 777,
            }
        }
    });

    // Two deliveries with *different* webhook_ids but the same event_id.
    let ts = chrono::Utc::now().timestamp();
    let r1 = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        ts,
        body.clone(),
        None,
    );
    let r2 = signed_request(&format!("msg_{}", Uuid::new_v4().simple()), ts, body, None);
    assert_eq!(send(&app, r1).await, StatusCode::OK);
    assert_eq!(send(&app, r2).await, StatusCode::OK);

    let balance: i64 = sqlx::query_scalar("SELECT credits_balance FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(balance, 100 + 777);

    let tx_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transactions \
         WHERE account_id = $1 AND metadata->>'provider_event_id' = $2",
    )
    .bind(account_id)
    .bind(&event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tx_count, 1);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn invoice_settled_clears_payment_method_status() {
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    // Pre-flag the account so we can verify the clearing side effect.
    sqlx::query("UPDATE accounts SET payment_method_status = 'errored' WHERE id = $1")
        .bind(account_id)
        .execute(&pool)
        .await
        .unwrap();

    let app = router(pool.clone());

    let event_id = format!("inv_{}", Uuid::new_v4().simple());
    let body = serde_json::json!({
        "type": "invoice.settled",
        "data": {
            "object": {
                "id": event_id,
                "customer_id": customer_id,
                "amount": 2500,
                "currency": "USD",
            }
        }
    });
    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        chrono::Utc::now().timestamp(),
        body,
        None,
    );
    assert_eq!(send(&app, req).await, StatusCode::OK);

    let status: Option<String> =
        sqlx::query_scalar("SELECT payment_method_status FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, None, "invoice.settled should clear the flag");

    // Transaction row landed with amount=0 and processor_amount stashed in metadata.
    let row = sqlx::query(
        "SELECT type, amount, metadata FROM transactions \
         WHERE account_id = $1 AND metadata->>'provider_event_id' = $2",
    )
    .bind(account_id)
    .bind(&event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("type"), "invoice_settled");
    assert_eq!(row.get::<i64, _>("amount"), 0);
    let meta: serde_json::Value = row.get("metadata");
    assert_eq!(meta["processor_amount"], 2500);
    assert_eq!(meta["currency"], "USD");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn invoice_errored_records_transaction_and_keeps_flag() {
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    // Errored flag should survive an invoice.errored — only settled clears.
    sqlx::query("UPDATE accounts SET payment_method_status = 'errored' WHERE id = $1")
        .bind(account_id)
        .execute(&pool)
        .await
        .unwrap();

    let app = router(pool.clone());

    let event_id = format!("inv_{}", Uuid::new_v4().simple());
    let body = serde_json::json!({
        "type": "invoice.errored",
        "data": {
            "object": {
                "id": event_id,
                "customer_id": customer_id,
                "amount": 1000,
                "currency": "USD",
            }
        }
    });
    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        chrono::Utc::now().timestamp(),
        body,
        None,
    );
    assert_eq!(send(&app, req).await, StatusCode::OK);

    let status: Option<String> =
        sqlx::query_scalar("SELECT payment_method_status FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status.as_deref(), Some("errored"));

    let tx_type: String = sqlx::query_scalar(
        "SELECT type FROM transactions \
         WHERE account_id = $1 AND metadata->>'provider_event_id' = $2",
    )
    .bind(account_id)
    .bind(&event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tx_type, "invoice_errored");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn payment_method_expired_flags_account() {
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    let app = router(pool.clone());

    let body = serde_json::json!({
        "type": "payment_method.expired",
        "data": {
            "object": {
                "id": format!("pm_{}", Uuid::new_v4().simple()),
                "customer_id": customer_id,
            }
        }
    });
    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        chrono::Utc::now().timestamp(),
        body,
        None,
    );
    assert_eq!(send(&app, req).await, StatusCode::OK);

    let status: Option<String> =
        sqlx::query_scalar("SELECT payment_method_status FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status.as_deref(), Some("expired"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn subscription_cancelled_deactivates_account() {
    let Some(pool) = try_pool().await else { return };
    let (account_id, customer_id) = seed_account(&pool).await;
    let app = router(pool.clone());

    let body = serde_json::json!({
        "type": "subscription.cancelled",
        "data": {
            "object": {
                "id": format!("sub_{}", Uuid::new_v4().simple()),
                "customer_id": customer_id,
            }
        }
    });
    let req = signed_request(
        &format!("msg_{}", Uuid::new_v4().simple()),
        chrono::Utc::now().timestamp(),
        body,
        None,
    );
    assert_eq!(send(&app, req).await, StatusCode::OK);

    let active: bool = sqlx::query_scalar("SELECT active FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!active);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires TEST_DATABASE_URL"]
async fn unknown_event_type_is_acked_and_logged() {
    let Some(pool) = try_pool().await else { return };
    let (_, customer_id) = seed_account(&pool).await;
    let app = router(pool.clone());

    let webhook_id = format!("msg_{}", Uuid::new_v4().simple());
    let body = serde_json::json!({
        "type": "some.future.event",
        "data": {
            "object": {
                "id": "evt_unknown",
                "customer_id": customer_id,
            }
        }
    });
    let req = signed_request(&webhook_id, chrono::Utc::now().timestamp(), body, None);
    // Must 200 — a non-2xx would cause Hyperline to retry forever.
    assert_eq!(send(&app, req).await, StatusCode::OK);

    // Row landed in the log for audit purposes.
    let logged: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM hyperline_webhook_log WHERE webhook_id = $1)",
    )
    .bind(&webhook_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(logged);
}
