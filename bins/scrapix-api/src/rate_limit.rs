//! Redis-backed rate limiting middleware.
//!
//! Uses a sliding window counter per account (or per IP for unauthenticated
//! requests) with per-tier limits from [`BillingTier::rate_limit`].

use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use redis::AsyncCommands;
use serde::Serialize;
use tracing::{debug, warn};

use crate::auth::AuthenticatedAccount;
use scrapix_core::BillingTier;

/// Shared rate limiter state.
#[derive(Clone)]
pub struct RateLimitState {
    redis: redis::aio::ConnectionManager,
}

impl RateLimitState {
    /// Connect to Redis. Returns `None` if connection fails.
    pub async fn new(redis_url: &str) -> Option<Self> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| warn!(error = %e, "Rate limiter: invalid Redis URL"))
            .ok()?;

        let conn = redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|e| warn!(error = %e, "Rate limiter: failed to connect to Redis"))
            .ok()?;

        Some(Self { redis: conn })
    }
}

#[derive(Serialize)]
struct RateLimitError {
    error: String,
    code: String,
    retry_after_seconds: u64,
}

/// Axum middleware that enforces per-account (or per-IP) rate limits.
///
/// When an `AuthenticatedAccount` extension is present, the account's billing
/// tier determines the requests-per-minute limit. Otherwise, a default of
/// 10 req/min per IP is applied.
///
/// Uses a Redis sliding window: `INCR` + `EXPIRE 60s` on a key like
/// `ratelimit:account:{id}` or `ratelimit:ip:{addr}`.
pub async fn rate_limit_middleware(
    State(rl): State<Arc<RateLimitState>>,
    request: Request,
    next: Next,
) -> Response {
    // Determine key and limit based on auth context
    let (key, limit) = if let Some(acct) = request.extensions().get::<AuthenticatedAccount>() {
        let tier: BillingTier = acct.tier.parse().unwrap_or_default();
        let rpm = tier.rate_limit() as u64;
        (format!("ratelimit:account:{}", acct.account_id), rpm)
    } else {
        // Fall back to IP-based limiting for unauthenticated requests
        let ip = request
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        (format!("ratelimit:ip:{}", ip), 10u64)
    };

    // Sliding window counter (1-minute window)
    let mut conn = rl.redis.clone();
    let count: u64 = match redis::pipe()
        .atomic()
        .cmd("INCR")
        .arg(&key)
        .cmd("EXPIRE")
        .arg(&key)
        .arg(60_i64)
        .ignore()
        .query_async::<Vec<u64>>(&mut conn)
        .await
    {
        Ok(results) => results.first().copied().unwrap_or(0),
        Err(e) => {
            // If Redis is down, allow the request (fail-open)
            debug!(error = %e, "Rate limiter: Redis error, allowing request");
            return next.run(request).await;
        }
    };

    if count > limit {
        let remaining_secs = {
            let ttl: i64 = conn.ttl(&key).await.unwrap_or(60);
            ttl.max(1) as u64
        };

        debug!(key = %key, count, limit, "Rate limit exceeded");

        return (
            StatusCode::TOO_MANY_REQUESTS,
            [
                ("retry-after", remaining_secs.to_string()),
                ("x-ratelimit-limit", limit.to_string()),
                ("x-ratelimit-remaining", "0".to_string()),
            ],
            Json(RateLimitError {
                error: format!(
                    "Rate limit exceeded. {} requests per minute allowed.",
                    limit
                ),
                code: "rate_limit_exceeded".to_string(),
                retry_after_seconds: remaining_secs,
            }),
        )
            .into_response();
    }

    // Add rate limit headers to successful responses
    let remaining = limit.saturating_sub(count);
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    if let Ok(v) = limit.to_string().parse() {
        headers.insert("x-ratelimit-limit", v);
    }
    if let Ok(v) = remaining.to_string().parse() {
        headers.insert("x-ratelimit-remaining", v);
    }

    response
}

/// Stricter rate limit for auth endpoints (login, signup) — 5 req/min per IP.
pub async fn auth_rate_limit_middleware(
    State(rl): State<Arc<RateLimitState>>,
    request: Request,
    next: Next,
) -> Response {
    let ip = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let key = format!("ratelimit:auth:{}", ip);
    let limit: u64 = 5; // 5 attempts per minute for auth

    let mut conn = rl.redis.clone();
    let count: u64 = match redis::pipe()
        .atomic()
        .cmd("INCR")
        .arg(&key)
        .cmd("EXPIRE")
        .arg(&key)
        .arg(60_i64)
        .ignore()
        .query_async::<Vec<u64>>(&mut conn)
        .await
    {
        Ok(results) => results.first().copied().unwrap_or(0),
        Err(e) => {
            debug!(error = %e, "Rate limiter: Redis error, allowing request");
            return next.run(request).await;
        }
    };

    if count > limit {
        let remaining_secs: u64 = conn.ttl::<_, i64>(&key).await.unwrap_or(60).max(1) as u64;

        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", remaining_secs.to_string())],
            Json(RateLimitError {
                error: "Too many authentication attempts. Please try again later.".to_string(),
                code: "rate_limit_exceeded".to_string(),
                retry_after_seconds: remaining_secs,
            }),
        )
            .into_response();
    }

    next.run(request).await
}
