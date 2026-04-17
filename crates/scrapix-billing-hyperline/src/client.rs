use std::time::Duration;

use reqwest::header;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::debug;
use url::Url;

use crate::config::HyperlineConfig;
use crate::error::HyperlineError;
use crate::events::UsageEvent;

/// Envelope returned by Hyperline list endpoints: `{ meta, data }`.
#[derive(Debug, Deserialize)]
pub struct ListResponse<T> {
    pub meta: ListMeta,
    pub data: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub struct ListMeta {
    pub total: u64,
    pub taken: u64,
    pub skipped: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Customer {
    pub id: String,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HyperlineClient {
    http: reqwest::Client,
    config: HyperlineConfig,
}

impl HyperlineClient {
    pub fn new(config: HyperlineConfig) -> Result<Self, HyperlineError> {
        let mut headers = header::HeaderMap::new();
        let auth_value = format!("Bearer {}", config.api_key);
        let mut auth = header::HeaderValue::from_str(&auth_value)
            .map_err(|e| HyperlineError::InvalidConfig(format!("invalid api key: {e}")))?;
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()?;

        Ok(Self { http, config })
    }

    pub fn from_env() -> Result<Self, HyperlineError> {
        Self::new(HyperlineConfig::from_env()?)
    }

    pub fn config(&self) -> &HyperlineConfig {
        &self.config
    }

    fn api_url(&self, path: &str) -> Result<Url, HyperlineError> {
        let trimmed = path.trim_start_matches('/');
        Ok(self.config.api_base.join(trimmed)?)
    }

    fn ingest_url(&self, path: &str) -> Result<Url, HyperlineError> {
        let trimmed = path.trim_start_matches('/');
        Ok(self.config.ingest_base.join(trimmed)?)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, HyperlineError> {
        let url = self.api_url(path)?;
        debug!(%url, "hyperline GET");
        let resp = self.http.get(url).query(query).send().await?;
        parse::<T>(resp).await
    }

    /// POSTs a batch of usage events to the ingest API. Hyperline accepts
    /// either a single event or an array; we always send an array so batching
    /// is a no-op change at the call site.
    pub async fn ingest_events(&self, events: &[UsageEvent<'_>]) -> Result<(), HyperlineError> {
        if events.is_empty() {
            return Ok(());
        }
        let url = self.ingest_url("/v1/events")?;
        debug!(%url, count = events.len(), "hyperline ingest");
        let resp = self.http.post(url).json(events).send().await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let bytes = resp.bytes().await?;
        let message = std::str::from_utf8(&bytes)
            .unwrap_or("<non-utf8 body>")
            .to_string();
        Err(HyperlineError::Api {
            status: status.as_u16(),
            message: truncate(&message, 512),
        })
    }

    /// Lists customers. Useful as a connectivity check.
    pub async fn list_customers(
        &self,
        limit: u32,
    ) -> Result<ListResponse<Customer>, HyperlineError> {
        let limit_str = limit.to_string();
        self.get_json("/v1/customers", &[("limit", &limit_str)])
            .await
    }

    /// Lightweight liveness probe — `GET /v1/customers?limit=1`.
    pub async fn ping(&self) -> Result<(), HyperlineError> {
        let _ = self.list_customers(1).await?;
        Ok(())
    }
}

async fn parse<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, HyperlineError> {
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if status.is_success() {
        let value: T = serde_json::from_slice(&bytes)?;
        return Ok(value);
    }
    let message = std::str::from_utf8(&bytes)
        .unwrap_or("<non-utf8 body>")
        .to_string();
    Err(HyperlineError::Api {
        status: status.as_u16(),
        message: truncate(&message, 512),
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
