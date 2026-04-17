//! Idempotent seed script: creates one `dynamic` product per
//! [`BillingEventType`] in the Hyperline workspace configured via
//! `HYPERLINE_API_KEY` + `HYPERLINE_API_BASE`.
//!
//! Each product is a `dynamic` billing primitive whose aggregator sums the
//! `credits` property across incoming events of the matching `event_type`:
//!
//! ```json
//! {
//!   "type": "dynamic",
//!   "name": "Page crawled",
//!   "unit_name": "credit",
//!   "aggregator": {
//!     "entity": "page_crawled",
//!     "operation": "sum",
//!     "property": "credits",
//!     "type": "metered"
//!   }
//! }
//! ```
//!
//! Idempotency: we first `GET /v1/products?name__equals=<display-name>` and
//! skip creation if any match is returned. Hyperline does not expose an
//! `external_id` on products, so name-equality is the only stable key we have.
//!
//! Usage:
//!
//! ```sh
//! HYPERLINE_API_KEY=test_… cargo run -p scrapix-billing-hyperline \
//!     --example seed_hyperline_products
//! ```

use std::error::Error;

use scrapix_billing_hyperline::client::HyperlineClient;
use scrapix_billing_hyperline::events::BillingEventType;
use serde::Deserialize;
use serde_json::json;

/// Display name + slug used for the Hyperline product backing a given event.
fn product_for(ev: BillingEventType) -> (&'static str, &'static str) {
    match ev {
        BillingEventType::PageCrawled => ("Page crawled", "page_crawled"),
        BillingEventType::BytesDownloaded => ("Bytes downloaded", "bytes_downloaded"),
        BillingEventType::JsRender => ("JS render", "js_render"),
        BillingEventType::ApiRequest => ("API request", "api_request"),
        BillingEventType::DocumentIndexed => ("Document indexed", "document_indexed"),
        BillingEventType::FeatureFormat => ("Feature format", "feature_format"),
        BillingEventType::AiFeature => ("AI feature", "ai_feature"),
    }
}

#[derive(Debug, Deserialize)]
struct ProductSummary {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProductList {
    data: Vec<ProductSummary>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let client = HyperlineClient::from_env()?;
    let target = if client.config().is_sandbox() {
        "sandbox"
    } else {
        "PRODUCTION"
    };
    println!("Seeding Hyperline products into {target}…");

    let mut created = 0usize;
    let mut skipped = 0usize;

    for ev in BillingEventType::all().iter().copied() {
        let (name, slug) = product_for(ev);

        // 1. Look up existing products by exact name.
        let existing: ProductList = client
            .get_json(
                "/v1/products",
                &[("name__equals", name), ("status", "active")],
            )
            .await?;
        if let Some(p) = existing.data.first() {
            println!(
                "  [=] {slug:<18} exists ({}{})",
                p.id,
                p.name
                    .as_deref()
                    .map(|n| format!(" — \"{n}\""))
                    .unwrap_or_default()
            );
            skipped += 1;
            continue;
        }

        // 2. Create the dynamic product. `unit_name` is shown on invoices.
        //    The `price_configurations` array is mandatory; we seed a single
        //    zero-amount volume tier (USD, monthly, 0 → ∞) so the product is
        //    valid in the dashboard but doesn't actually bill anything. Real
        //    pricing is set up by ops in the Hyperline UI post-cutover.
        let body = json!({
            "type": "dynamic",
            "name": name,
            "description": format!("Scrapix billable event: {slug}"),
            "unit_name": "credit",
            "is_available_on_demand": true,
            "is_available_on_subscription": true,
            "aggregator": {
                "entity": slug,
                "operation": "sum",
                "property": "credits",
                "type": "metered",
            },
            "price_configurations": [
                {
                    "currency": "USD",
                    "billing_interval": { "period": "months", "count": 1 },
                    "type": "volume",
                    "prices": [
                        {
                            "type": "volume",
                            "amount": 0,
                            "from": 0,
                            "to": null,
                        },
                    ],
                },
            ],
        });

        let created_product: ProductSummary = client.post_json("/v1/products", &body).await?;
        println!("  [+] {slug:<18} created ({})", created_product.id);
        created += 1;
    }

    println!("Done. created={created} skipped={skipped}");
    Ok(())
}
