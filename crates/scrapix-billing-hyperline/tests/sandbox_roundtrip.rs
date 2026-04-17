//! Live sandbox round-trip.
//!
//! Skipped unless `HYPERLINE_API_KEY` is set. Run with:
//!
//! ```sh
//! cargo test -p scrapix-billing-hyperline --test sandbox_roundtrip -- --ignored
//! ```

use scrapix_billing_hyperline::HyperlineClient;

#[tokio::test]
#[ignore = "requires HYPERLINE_API_KEY to hit live sandbox"]
async fn lists_customers_in_sandbox() {
    let client = HyperlineClient::from_env().expect("HYPERLINE_API_KEY must be set");
    assert!(
        client.config().is_sandbox(),
        "test must run against a test_ key"
    );

    let page = client
        .list_customers(1)
        .await
        .expect("list_customers failed");
    // Meta fields are always present; data may be empty on a fresh sandbox.
    assert!(page.meta.taken <= 1, "limit=1 was not honored");
}
