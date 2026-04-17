//! Usage-event catalog and outbox enqueue helper.
//!
//! Events are written to `hyperline_events_outbox` in the same Postgres
//! transaction as the credit-ledger debit, so a debited credit always has a
//! matching outbox row. A background drain worker (see `outbox.rs`) POSTs the
//! rows to Hyperline and uses the outbox row's UUID as the `record.id` —
//! giving us free idempotency across retries.
//!
//! Wire format matches Hyperline's `BillableEvent` schema:
//! ```json
//! {
//!   "customer_id": "cus_…",
//!   "event_type": "api_request",
//!   "timestamp": "2026-04-17T…Z",
//!   "record": { "id": "<outbox-uuid>", "credits": 5, …extra scalars }
//! }
//! ```
//! `record.id` is the dedupe key on Hyperline's side; arbitrary scalars in
//! `record` become aggregatable properties (e.g. `sum(record.credits)` for
//! a dynamic product).

use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::error::HyperlineError;

/// Event types mirroring the existing credit-cost rules in
/// `crates/scrapix-billing/src/credits.rs`. One variant per rule so Hyperline
/// can price each independently without us redeploying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingEventType {
    PageCrawled,
    BytesDownloaded,
    JsRender,
    ApiRequest,
    DocumentIndexed,
    FeatureFormat,
    AiFeature,
}

impl BillingEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PageCrawled => "page_crawled",
            Self::BytesDownloaded => "bytes_downloaded",
            Self::JsRender => "js_render",
            Self::ApiRequest => "api_request",
            Self::DocumentIndexed => "document_indexed",
            Self::FeatureFormat => "feature_format",
            Self::AiFeature => "ai_feature",
        }
    }

    /// All variants, for bootstrap/seed tooling.
    pub fn all() -> &'static [Self] {
        &[
            Self::PageCrawled,
            Self::BytesDownloaded,
            Self::JsRender,
            Self::ApiRequest,
            Self::DocumentIndexed,
            Self::FeatureFormat,
            Self::AiFeature,
        ]
    }
}

/// Wire shape posted to Hyperline's ingest API. `record` is a pre-built JSON
/// object containing `id` (dedupe key), `credits` (aggregator property), and
/// any flattened metadata scalars — see [`build_record`].
#[derive(Debug, Serialize)]
pub struct UsageEvent<'a> {
    pub customer_id: &'a str,
    pub event_type: &'a str,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub record: serde_json::Value,
}

/// Build the `record` JSON object for a `UsageEvent`.
///
/// - `id` becomes Hyperline's dedupe key (`record.id`).
/// - `credits` is exposed as an aggregatable numeric property.
/// - Any scalar keys in `metadata` (strings, numbers, booleans, nulls) are
///   flattened into the record. Non-scalar values are dropped to stay within
///   the `BillableEvent.record` `additionalProperties` shape accepted by
///   Hyperline. Existing keys (`id`, `credits`) are never overwritten.
pub fn build_record(
    id: &str,
    credits: f64,
    metadata: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    obj.insert("credits".to_string(), serde_json::Value::from(credits));
    if let Some(serde_json::Value::Object(meta)) = metadata {
        for (k, v) in meta {
            if obj.contains_key(k) {
                continue;
            }
            if v.is_string() || v.is_number() || v.is_boolean() || v.is_null() {
                obj.insert(k.clone(), v.clone());
            }
        }
    }
    serde_json::Value::Object(obj)
}

/// Inline payload persisted in the outbox. The drain worker reads this back
/// and builds a `UsageEvent.record` at send time via [`build_record`].
#[derive(Debug, Serialize, Deserialize)]
pub struct OutboxPayload {
    pub quantity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Enqueue a usage event on the outbox. Accepts any `PgExecutor`, so it can
/// be called either on a pool (standalone) or — crucially — on
/// `&mut *transaction` to stay atomic with the ledger debit.
pub async fn enqueue_usage_event<'c, E>(
    executor: E,
    account_id: Uuid,
    event_type: BillingEventType,
    quantity: f64,
    metadata: Option<serde_json::Value>,
) -> Result<Uuid, HyperlineError>
where
    E: PgExecutor<'c>,
{
    let id = Uuid::new_v4();
    let payload = serde_json::to_value(OutboxPayload { quantity, metadata })?;

    sqlx::query(
        "INSERT INTO hyperline_events_outbox (id, account_id, event_type, payload) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(account_id)
    .bind(event_type.as_str())
    .bind(payload)
    .execute(executor)
    .await?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_record_minimal() {
        let rec = build_record("abc", 5.0, None);
        assert_eq!(rec["id"], "abc");
        assert_eq!(rec["credits"], 5.0);
        assert_eq!(rec.as_object().unwrap().len(), 2);
    }

    #[test]
    fn build_record_flattens_scalar_metadata() {
        let meta = serde_json::json!({
            "operation": "scrape",
            "description": "…",
            "js_rendered": true,
        });
        let rec = build_record("abc", 3.0, Some(&meta));
        assert_eq!(rec["operation"], "scrape");
        assert_eq!(rec["description"], "…");
        assert_eq!(rec["js_rendered"], true);
    }

    #[test]
    fn build_record_drops_non_scalar_metadata() {
        let meta = serde_json::json!({
            "nested": { "a": 1 },
            "arr": [1, 2, 3],
            "keep_me": "yes",
        });
        let rec = build_record("abc", 1.0, Some(&meta));
        assert!(rec.get("nested").is_none());
        assert!(rec.get("arr").is_none());
        assert_eq!(rec["keep_me"], "yes");
    }

    #[test]
    fn build_record_never_overwrites_reserved_keys() {
        let meta = serde_json::json!({
            "id": "shadow",
            "credits": 999,
            "real": "kept",
        });
        let rec = build_record("outbox-id", 1.0, Some(&meta));
        assert_eq!(rec["id"], "outbox-id");
        assert_eq!(rec["credits"], 1.0);
        assert_eq!(rec["real"], "kept");
    }
}
