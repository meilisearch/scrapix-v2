//! Idempotent product seeding for Hyperline.
//!
//! Hyperline's products + pricing live in the Hyperline dashboard and have
//! historically been a manual ops step before each deploy that introduces a
//! new `BillingEventType` variant. When the step is missed, usage events
//! 4xx at the ingest endpoint and the outbox backs up (see the runbook in
//! `docs/operations/hyperline-billing.mdx`).
//!
//! This module exposes the same seeding logic the `seed_hyperline_products`
//! example uses, but as a library function so the API can optionally run it
//! at boot (gated on `HYPERLINE_AUTO_SEED`, opt-in by default).
//!
//! ## Idempotency
//!
//! Hyperline does not expose an `external_id` on products, so name-equality
//! is the only stable lookup key. Each call first issues
//! `GET /v1/products?name__equals=<display name>&status=active` and skips
//! creation if a match exists. A partially-completed seed run is safe to
//! re-run; only the missing products are created.
//!
//! ## Pricing
//!
//! Seeded products are `dynamic` (metered) and use a single placeholder
//! volume tier at `amount: 0` in USD. This is intentional — the goal here
//! is to get the product *registered* so events stop 4xxing. Real prices
//! are still set by ops in the Hyperline UI after the seed.

use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use crate::client::HyperlineClient;
use crate::error::HyperlineError;
use crate::events::BillingEventType;

/// Display name + entity slug for the Hyperline product backing `ev`.
///
/// The slug must equal `BillingEventType::as_str()` so the dynamic
/// aggregator's `entity` field matches the `event_type` posted to
/// `/v1/events` at ingest time. The unit test below enforces that.
pub fn product_for(ev: BillingEventType) -> (&'static str, &'static str) {
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

/// Summary of a [`seed_products`] run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SeedReport {
    pub created: usize,
    pub skipped: usize,
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

/// Idempotently create one `dynamic` Hyperline product per
/// `BillingEventType` variant.
///
/// Looks up each product by exact name match before posting. Returns the
/// counts of created vs. skipped products. Any Hyperline error short-circuits
/// the run — callers that want log-and-continue semantics should wrap with
/// [`seed_if_enabled`] (or `let _ = seed_products(&client).await`).
pub async fn seed_products(client: &HyperlineClient) -> Result<SeedReport, HyperlineError> {
    let mut report = SeedReport::default();

    for ev in BillingEventType::all().iter().copied() {
        let (name, slug) = product_for(ev);

        let existing: ProductList = client
            .get_json(
                "/v1/products",
                &[("name__equals", name), ("status", "active")],
            )
            .await?;
        if let Some(p) = existing.data.first() {
            info!(
                event_type = slug,
                product_id = %p.id,
                product_name = p.name.as_deref().unwrap_or(name),
                "Hyperline product already exists — skipping"
            );
            report.skipped += 1;
            continue;
        }

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

        let created: ProductSummary = client.post_json("/v1/products", &body).await?;
        info!(
            event_type = slug,
            product_id = %created.id,
            "Hyperline product created (placeholder zero-amount pricing — set real prices in the dashboard)"
        );
        report.created += 1;
    }

    Ok(report)
}

/// Returns true when the `HYPERLINE_AUTO_SEED` env var is set to `1`/`true`
/// (case-insensitive, with surrounding whitespace trimmed). Any other value
/// — including unset — returns false.
pub fn auto_seed_enabled() -> bool {
    match std::env::var("HYPERLINE_AUTO_SEED") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        }
        Err(_) => false,
    }
}

/// Boot-time wrapper: if `HYPERLINE_AUTO_SEED` is set, run [`seed_products`]
/// and log the outcome. Failures are downgraded to a `WARN` and swallowed —
/// the API must not crashloop because of a Hyperline outage or a transient
/// product-API hiccup. The outbox keeps enqueuing either way; events will
/// still 4xx until products exist, but ops can re-run the example script
/// manually if the auto-seed degraded.
pub async fn seed_if_enabled(client: &HyperlineClient) {
    if !auto_seed_enabled() {
        return;
    }

    info!(
        sandbox = client.config().is_sandbox(),
        "HYPERLINE_AUTO_SEED is set — ensuring metered products exist in Hyperline"
    );

    match seed_products(client).await {
        Ok(report) => {
            info!(
                created = report.created,
                skipped = report.skipped,
                sandbox = client.config().is_sandbox(),
                "Hyperline auto-seed completed"
            );
        }
        Err(e) => {
            warn!(
                error = %e,
                sandbox = client.config().is_sandbox(),
                "Hyperline auto-seed failed — set real prices manually or re-run `cargo run -p scrapix-billing-hyperline --example seed_hyperline_products`. Events will 4xx until products exist."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_slug_matches_event_type() {
        // The dynamic aggregator's `entity` field is the slug, and Hyperline
        // matches it against the `event_type` we POST at ingest. If these
        // ever drift the events 4xx silently from the customer's POV.
        for ev in BillingEventType::all().iter().copied() {
            let (_name, slug) = product_for(ev);
            assert_eq!(
                slug,
                ev.as_str(),
                "product_for({ev:?}) slug must equal BillingEventType::as_str()"
            );
        }
    }

    #[test]
    fn product_for_covers_every_variant() {
        // Catches the "added a variant, forgot the product mapping" mistake.
        for ev in BillingEventType::all().iter().copied() {
            let (name, slug) = product_for(ev);
            assert!(!name.is_empty(), "missing display name for {ev:?}");
            assert!(!slug.is_empty(), "missing slug for {ev:?}");
        }
    }

    #[test]
    fn auto_seed_enabled_reads_env_var() {
        // SAFETY: tests mutate process env; restore at the end.
        let prev = std::env::var("HYPERLINE_AUTO_SEED").ok();

        for (val, expected) in [
            ("1", true),
            ("true", true),
            ("TRUE", true),
            (" 1 ", true),
            ("0", false),
            ("false", false),
            ("yes", false),
            ("", false),
        ] {
            unsafe {
                std::env::set_var("HYPERLINE_AUTO_SEED", val);
            }
            assert_eq!(
                auto_seed_enabled(),
                expected,
                "HYPERLINE_AUTO_SEED={val:?} should be {expected}"
            );
        }

        unsafe {
            std::env::remove_var("HYPERLINE_AUTO_SEED");
        }
        assert!(!auto_seed_enabled(), "unset should be false");

        // Restore.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("HYPERLINE_AUTO_SEED", v),
                None => std::env::remove_var("HYPERLINE_AUTO_SEED"),
            }
        }
    }
}
