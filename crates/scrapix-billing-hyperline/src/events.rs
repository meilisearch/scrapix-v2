//! Usage-event catalog and outbox enqueue helper.
//!
//! Events are written to `hyperline_events_outbox` in the same Postgres
//! transaction as the credit-ledger debit, so a debited credit always has a
//! matching outbox row. A background drain worker (see `outbox.rs`) POSTs the
//! rows to Hyperline and uses the outbox row's UUID as Hyperline's
//! `record_id` — giving us free idempotency across retries.

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
}

/// Payload shape posted to `POST /v1/events`. `record_id` is the outbox row
/// UUID so Hyperline dedupes retries.
#[derive(Debug, Serialize)]
pub struct UsageEvent<'a> {
    pub record_id: &'a str,
    pub customer_id: &'a str,
    pub event_type: &'a str,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub quantity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Inline payload persisted in the outbox. The drain worker reads this back
/// and builds a `UsageEvent` at send time.
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
