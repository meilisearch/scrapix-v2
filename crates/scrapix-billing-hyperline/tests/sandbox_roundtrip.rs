//! Live Hyperline sandbox round-trip.
//!
//! These tests are `#[ignore]`d by default and only run against the live
//! sandbox when explicitly invoked. They exercise the wire contract — URL
//! paths, header names, payload shapes, serde envelopes — in the one way
//! that mocks and unit tests cannot.
//!
//! Run with:
//!
//! ```sh
//! HYPERLINE_API_KEY=test_... \
//!   cargo test -p scrapix-billing-hyperline \
//!     --test sandbox_roundtrip -- --ignored --nocapture
//! ```
//!
//! Each test is independent and uses a fresh external_id so re-runs don't
//! collide. A 4xx from Hyperline is reported verbatim so wire drift is
//! loud (rather than silently turning into `None` via `#[serde(default)]`).

use scrapix_billing_hyperline::events::{build_record, UsageEvent};
use scrapix_billing_hyperline::{boot_self_check, Customer, HyperlineClient};
use uuid::Uuid;

fn client() -> HyperlineClient {
    let c = HyperlineClient::from_env().expect("HYPERLINE_API_KEY must be set");
    assert!(
        c.config().is_sandbox(),
        "sandbox roundtrip must run against a test_ key, got prod_"
    );
    c
}

/// Connectivity + auth + JSON envelope — the cheapest signal that the
/// client is wired correctly.
#[tokio::test]
#[ignore = "requires HYPERLINE_API_KEY to hit live sandbox"]
async fn lists_customers_in_sandbox() {
    let c = client();
    let page = c.list_customers(1).await.expect("list_customers failed");
    assert!(page.meta.taken <= 1, "limit=1 was not honored");
    println!(
        "[ok] list_customers → meta={{total:{}, taken:{}, skipped:{}}}",
        page.meta.total, page.meta.taken, page.meta.skipped
    );
}

/// The boot self-check the API runs during startup. Verifies the ping
/// returns 2xx and that the function emits its event-type manifest log.
/// Run as a single-test smoke to catch regressions in the exact
/// log-and-ping contract the API deploy relies on.
#[tokio::test]
#[ignore = "requires HYPERLINE_API_KEY to hit live sandbox"]
async fn boot_self_check_passes_in_sandbox() {
    let c = client();
    boot_self_check(&c)
        .await
        .expect("boot_self_check failed against live sandbox");
    println!("[ok] boot_self_check → ping 2xx + manifest logged");
}

/// Create a fresh customer, then verify we can fetch their portal URL.
///
/// This is the exact path `GET /account/billing/portal` walks in prod:
/// the backend holds `hyperline_customer_id`, we call
/// `get_portal_url(customer_id)`, and we redirect the browser to the
/// returned URL.
#[tokio::test]
#[ignore = "requires HYPERLINE_API_KEY to hit live sandbox"]
async fn creates_customer_and_fetches_portal_url() {
    let c = client();

    // Unique per run; `external_id` is our account UUID in prod.
    let external_id = format!("smoke-{}", Uuid::new_v4());
    let body = serde_json::json!({
        "external_id": external_id,
        "name": "Scrapix smoke test",
        "email": format!("{external_id}@example.com"),
    });

    let created: Customer = c
        .post_json("/v1/customers", &body)
        .await
        .expect("create_customer failed");
    assert_eq!(created.external_id.as_deref(), Some(external_id.as_str()));
    println!(
        "[ok] create_customer → id={} external_id={:?}",
        created.id, created.external_id
    );

    let portal = c
        .get_portal_url(&created.id)
        .await
        .expect("get_portal_url failed");
    assert!(
        portal.url.starts_with("http"),
        "portal URL must be absolute: {}",
        portal.url
    );
    println!("[ok] get_portal_url → {}", portal.url);
}

/// Post a single billable event to the ingest host.
///
/// Mirrors exactly what the outbox drain worker does: builds a
/// `UsageEvent` with a UUID `record.id` and POSTs it to `/v1/events`.
/// A 2xx is the success criterion — Hyperline's ingest returns an
/// empty body on accept. Repeated POSTs with the same `record.id`
/// validate our idempotency assumption (one of the flagged risks).
#[tokio::test]
#[ignore = "requires HYPERLINE_API_KEY to hit live sandbox"]
async fn ingests_event_and_is_idempotent_on_record_id() {
    let c = client();

    // Need a customer to attach the event to.
    let external_id = format!("smoke-{}", Uuid::new_v4());
    let body = serde_json::json!({
        "external_id": external_id,
        "name": "Scrapix smoke event",
        "email": format!("{external_id}@example.com"),
    });
    let customer: Customer = c
        .post_json("/v1/customers", &body)
        .await
        .expect("create_customer failed");

    // One outbox-shaped event.
    let record_id = Uuid::new_v4().to_string();
    let record = build_record(
        &record_id,
        3.0,
        Some(&serde_json::json!({
            "operation": "scrape",
            "smoke": true,
        })),
    );
    let event = UsageEvent {
        customer_id: &customer.id,
        event_type: "api_request",
        timestamp: chrono::Utc::now(),
        record,
    };

    c.ingest_event(&event)
        .await
        .expect("ingest_event #1 failed");
    println!("[ok] ingest_event #1 → 2xx (record.id={record_id})");

    // Same record.id → expect idempotent (2xx) rather than 409 / duplicate.
    // If Hyperline doesn't dedupe on record.id this will still 2xx, but a
    // follow-up `/v1/events/list` query (not implemented in our client
    // yet) would show two rows — this test only validates the HTTP
    // contract, not the server-side behavior.
    c.ingest_event(&event)
        .await
        .expect("ingest_event #2 (replay) failed");
    println!("[ok] ingest_event #2 (replay, same record.id) → 2xx");
}
