//! Usage event types emitted to Hyperline's ingest API.
//!
//! The actual emission path uses a Postgres outbox (see Linear SCR-68). This
//! module currently defines the event catalog; the outbox writer and drain
//! worker land in a follow-up change once the migration for
//! `hyperline_events_outbox` is applied.

use serde::{Deserialize, Serialize};

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

/// Payload shape posted to `POST /v1/events`. The outbox row UUID is used as
/// `record_id` to make retries idempotent.
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
