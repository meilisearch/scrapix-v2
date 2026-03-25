//! Redis-backed rate limiting middleware with in-memory fallback for auth endpoints.
//!
//! Uses a sliding window counter per account (or per IP for unauthenticated
//! requests) with per-tier limits from [`BillingTier::rate_limit`].
//!
//! Auth endpoints (login, signup, password reset) use a stricter limit and
//! an in-memory fallback so brute-force protection works even without Redis.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use dashmap::DashMap;
use redis::AsyncCommands;
use serde::Serialize;
use tracing::{debug, warn};

use crate::auth::AuthenticatedAccount;
use scrapix_core::BillingTier;

/// Shared rate limiter state.
#[derive(Clone)]
pub struct RateLimitState {
    redis: redis::aio::ConnectionManager,
    /// In-memory fallback for auth endpoints when Redis errors occur.
    auth_fallback: InMemoryAuthRateLimiter,
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

        Some(Self {
            redis: conn,
            auth_fallback: InMemoryAuthRateLimiter::new(),
        })
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

/// In-memory sliding window entry for auth rate limiting fallback.
struct AuthWindow {
    count: u64,
    window_start: Instant,
}

/// In-memory auth rate limiter that works without Redis.
/// Uses a lock-free concurrent hashmap so multiple requests can be checked
/// in parallel without contention on a single mutex.
#[derive(Clone, Default)]
pub struct InMemoryAuthRateLimiter {
    windows: Arc<DashMap<String, AuthWindow>>,
}

impl InMemoryAuthRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check and increment counter for an IP. Returns current count.
    fn check(&self, ip: &str, window_secs: u64) -> u64 {
        let now = Instant::now();

        // Evict expired entries periodically
        if self.windows.len() > 256 {
            self.windows
                .retain(|_, w| now.duration_since(w.window_start).as_secs() < window_secs);
        }

        let mut entry = self.windows.entry(ip.to_string()).or_insert(AuthWindow {
            count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(entry.window_start).as_secs() >= window_secs {
            entry.count = 0;
            entry.window_start = now;
        }

        entry.count += 1;
        entry.count
    }
}

const AUTH_RATE_LIMIT: u64 = 5; // 5 attempts per minute
const AUTH_WINDOW_SECS: u64 = 60;

/// Stricter rate limit for auth endpoints (login, signup) — 5 req/min per IP.
/// Uses Redis when available, falls back to in-memory counters.
/// Unlike the general rate limiter, auth rate limiting **fails closed** — if both
/// Redis and in-memory checks fail, the request is rejected.
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

    let mut conn = rl.redis.clone();
    let count: u64 = match redis::pipe()
        .atomic()
        .cmd("INCR")
        .arg(&key)
        .cmd("EXPIRE")
        .arg(&key)
        .arg(AUTH_WINDOW_SECS as i64)
        .ignore()
        .query_async::<Vec<u64>>(&mut conn)
        .await
    {
        Ok(results) => results.first().copied().unwrap_or(0),
        Err(e) => {
            // Redis failed — fall back to in-memory rate limiting (fail closed)
            warn!(error = %e, ip = %ip, "Auth rate limiter: Redis error, using in-memory fallback");
            rl.auth_fallback.check(&ip, AUTH_WINDOW_SECS)
        }
    };

    if count > AUTH_RATE_LIMIT {
        let remaining_secs: u64 = conn
            .ttl::<_, i64>(&key)
            .await
            .unwrap_or(AUTH_WINDOW_SECS as i64)
            .max(1) as u64;

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

/// Standalone in-memory auth rate limiter for when Redis is not configured at all.
/// Applied to auth routes so brute-force protection always works.
pub async fn auth_rate_limit_in_memory_middleware(
    State(limiter): State<Arc<InMemoryAuthRateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    let ip = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let count = limiter.check(&ip, AUTH_WINDOW_SECS);

    if count > AUTH_RATE_LIMIT {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", AUTH_WINDOW_SECS.to_string())],
            Json(RateLimitError {
                error: "Too many authentication attempts. Please try again later.".to_string(),
                code: "rate_limit_exceeded".to_string(),
                retry_after_seconds: AUTH_WINDOW_SECS,
            }),
        )
            .into_response();
    }

    next.run(request).await
}
