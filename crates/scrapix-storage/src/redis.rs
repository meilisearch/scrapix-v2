//! Redis storage backend for rate limiting and caching

use std::time::Duration;

use async_trait::async_trait;
use redis::{aio::ConnectionManager, AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use scrapix_core::{Result, ScrapixError};

/// Redis configuration
#[derive(Debug, Clone)]
pub struct RedisConfig {
    /// Redis URL (redis://host:port or rediss://host:port for TLS)
    pub url: String,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Default TTL for cached values
    pub default_ttl: Duration,
    /// Key prefix for namespacing
    pub key_prefix: String,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".to_string(),
            connect_timeout: Duration::from_secs(5),
            default_ttl: Duration::from_secs(3600),
            key_prefix: "scrapix:".to_string(),
        }
    }
}

/// Redis client wrapper
pub struct RedisStorage {
    config: RedisConfig,
    conn: ConnectionManager,
}

impl RedisStorage {
    /// Create a new Redis storage client
    pub async fn new(config: RedisConfig) -> Result<Self> {
        let client = Client::open(config.url.as_str())
            .map_err(|e| ScrapixError::Storage(format!("Failed to create Redis client: {}", e)))?;

        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Failed to connect to Redis: {}", e)))?;

        Ok(Self { config, conn })
    }

    /// Create a new Redis storage with default configuration
    pub async fn with_url(url: impl Into<String>) -> Result<Self> {
        Self::new(RedisConfig {
            url: url.into(),
            ..Default::default()
        })
        .await
    }

    /// Get the full key with prefix
    fn key(&self, key: &str) -> String {
        format!("{}{}", self.config.key_prefix, key)
    }

    /// Set a value with TTL
    pub async fn set(&self, key: &str, value: &str, ttl: Option<Duration>) -> Result<()> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);
        let ttl = ttl.unwrap_or(self.config.default_ttl);

        conn.set_ex::<_, _, ()>(&full_key, value, ttl.as_secs())
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis SET failed: {}", e)))?;

        Ok(())
    }

    /// Get a value
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);

        let value: Option<String> = conn
            .get(&full_key)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis GET failed: {}", e)))?;

        Ok(value)
    }

    /// Delete a key
    pub async fn delete(&self, key: &str) -> Result<bool> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);

        let deleted: i64 = conn
            .del(&full_key)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis DEL failed: {}", e)))?;

        Ok(deleted > 0)
    }

    /// Check if key exists
    pub async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);

        let exists: bool = conn
            .exists(&full_key)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis EXISTS failed: {}", e)))?;

        Ok(exists)
    }

    /// Increment a counter
    pub async fn incr(&self, key: &str) -> Result<i64> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);

        let value: i64 = conn
            .incr(&full_key, 1)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis INCR failed: {}", e)))?;

        Ok(value)
    }

    /// Set expiration on a key
    pub async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        let mut conn = self.conn.clone();
        let full_key = self.key(key);

        let result: bool = conn
            .expire(&full_key, ttl.as_secs() as i64)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis EXPIRE failed: {}", e)))?;

        Ok(result)
    }

    /// Health check
    pub async fn ping(&self) -> Result<bool> {
        let mut conn = self.conn.clone();

        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Redis PING failed: {}", e)))?;

        Ok(pong == "PONG")
    }
}

/// Token bucket rate limiter using Redis
pub struct RedisRateLimiter {
    storage: RedisStorage,
    config: RateLimiterConfig,
}

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Default requests per second
    pub default_rate: f64,
    /// Burst capacity (max tokens)
    pub burst_capacity: u32,
    /// Key prefix for rate limit keys
    pub key_prefix: String,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            default_rate: 1.0, // 1 request per second
            burst_capacity: 10,
            key_prefix: "ratelimit:".to_string(),
        }
    }
}

impl RedisRateLimiter {
    /// Create a new rate limiter
    pub fn new(storage: RedisStorage, config: RateLimiterConfig) -> Self {
        Self { storage, config }
    }

    /// Create a rate limiter with default configuration
    pub fn with_defaults(storage: RedisStorage) -> Self {
        Self::new(storage, RateLimiterConfig::default())
    }

    /// Get the rate limit key for a domain
    fn rate_key(&self, domain: &str) -> String {
        format!("{}tokens:{}", self.config.key_prefix, domain)
    }

    fn last_update_key(&self, domain: &str) -> String {
        format!("{}last:{}", self.config.key_prefix, domain)
    }

    fn rate_config_key(&self, domain: &str) -> String {
        format!("{}rate:{}", self.config.key_prefix, domain)
    }

    /// Acquire a token (wait if necessary)
    #[instrument(skip(self))]
    pub async fn acquire(&self, domain: &str) -> Result<()> {
        loop {
            match self.try_acquire(domain).await? {
                AcquireResult::Acquired => return Ok(()),
                AcquireResult::Wait(duration) => {
                    debug!(
                        domain,
                        wait_ms = duration.as_millis(),
                        "Rate limited, waiting"
                    );
                    tokio::time::sleep(duration).await;
                }
            }
        }
    }

    /// Try to acquire a token without waiting
    pub async fn try_acquire(&self, domain: &str) -> Result<AcquireResult> {
        let rate = self.get_rate(domain).await?;
        let now = chrono::Utc::now().timestamp_millis() as f64;

        let tokens_key = self.rate_key(domain);
        let last_key = self.last_update_key(domain);

        // Get current state
        let tokens: f64 = self
            .storage
            .get(&tokens_key)
            .await?
            .and_then(|s| s.parse().ok())
            .unwrap_or(self.config.burst_capacity as f64);

        let last_update: f64 = self
            .storage
            .get(&last_key)
            .await?
            .and_then(|s| s.parse().ok())
            .unwrap_or(now);

        // Calculate tokens to add based on time elapsed
        let elapsed_ms = (now - last_update).max(0.0);
        let new_tokens = elapsed_ms * rate / 1000.0;
        let available = (tokens + new_tokens).min(self.config.burst_capacity as f64);

        if available >= 1.0 {
            // Consume a token
            let remaining = available - 1.0;
            self.storage
                .set(
                    &tokens_key,
                    &remaining.to_string(),
                    Some(Duration::from_secs(60)),
                )
                .await?;
            self.storage
                .set(&last_key, &now.to_string(), Some(Duration::from_secs(60)))
                .await?;
            Ok(AcquireResult::Acquired)
        } else {
            // Calculate wait time for next token
            let needed = 1.0 - available;
            let wait_ms = (needed / rate * 1000.0) as u64;
            Ok(AcquireResult::Wait(Duration::from_millis(wait_ms.max(10))))
        }
    }

    /// Set rate limit for a domain
    pub async fn set_limit(&self, domain: &str, requests_per_second: f64) -> Result<()> {
        let key = self.rate_config_key(domain);
        self.storage
            .set(&key, &requests_per_second.to_string(), None)
            .await
    }

    /// Get rate limit for a domain
    pub async fn get_rate(&self, domain: &str) -> Result<f64> {
        let key = self.rate_config_key(domain);
        let rate = self
            .storage
            .get(&key)
            .await?
            .and_then(|s| s.parse().ok())
            .unwrap_or(self.config.default_rate);
        Ok(rate)
    }

    /// Reset rate limit state for a domain
    pub async fn reset(&self, domain: &str) -> Result<()> {
        self.storage.delete(&self.rate_key(domain)).await?;
        self.storage.delete(&self.last_update_key(domain)).await?;
        Ok(())
    }
}

/// Result of trying to acquire a rate limit token
#[derive(Debug)]
pub enum AcquireResult {
    /// Token acquired, proceed
    Acquired,
    /// Need to wait before trying again
    Wait(Duration),
}

/// Implementation of core RateLimiter trait
#[async_trait]
impl scrapix_core::traits::RateLimiter for RedisRateLimiter {
    async fn acquire(&self, domain: &str) -> Result<()> {
        RedisRateLimiter::acquire(self, domain).await
    }

    async fn set_limit(&self, domain: &str, requests_per_second: f64) -> Result<()> {
        RedisRateLimiter::set_limit(self, domain, requests_per_second).await
    }

    async fn get_rate(&self, domain: &str) -> Result<f64> {
        RedisRateLimiter::get_rate(self, domain).await
    }
}

/// URL seen cache using Redis
pub struct RedisSeenCache {
    storage: RedisStorage,
    prefix: String,
    ttl: Duration,
}

impl RedisSeenCache {
    /// Create a new seen cache
    pub fn new(storage: RedisStorage, prefix: impl Into<String>, ttl: Duration) -> Self {
        Self {
            storage,
            prefix: prefix.into(),
            ttl,
        }
    }

    /// Create a seen cache with default settings
    pub fn with_defaults(storage: RedisStorage) -> Self {
        Self::new(storage, "seen:", Duration::from_secs(86400 * 7)) // 7 days
    }

    /// Mark a URL as seen
    pub async fn mark_seen(&self, url: &str) -> Result<()> {
        let key = format!("{}{}", self.prefix, url_hash(url));
        self.storage.set(&key, "1", Some(self.ttl)).await
    }

    /// Check if a URL has been seen
    pub async fn is_seen(&self, url: &str) -> Result<bool> {
        let key = format!("{}{}", self.prefix, url_hash(url));
        self.storage.exists(&key).await
    }

    /// Mark multiple URLs as seen
    pub async fn mark_seen_batch(&self, urls: &[String]) -> Result<()> {
        for url in urls {
            self.mark_seen(url).await?;
        }
        Ok(())
    }
}

/// Crawl history record stored in Redis for incremental crawling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlHistoryRecord {
    /// ETag from the HTTP response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Last-Modified header from the HTTP response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Timestamp of the last successful crawl (epoch millis)
    pub crawled_at: i64,
}

/// Redis-backed crawl history store for incremental crawling.
///
/// Stores per-URL `{etag, last_modified, crawled_at}` keyed by `(index_uid, url)`.
/// The crawler worker looks up history before fetching to populate conditional HTTP
/// headers (If-None-Match / If-Modified-Since). After a successful fetch it saves
/// the response headers so the next crawl can skip unchanged pages.
pub struct RedisCrawlHistory {
    storage: RedisStorage,
    /// TTL for history records (default 30 days)
    ttl: Duration,
}

impl RedisCrawlHistory {
    /// Create a new crawl history store
    pub fn new(storage: RedisStorage, ttl: Duration) -> Self {
        Self { storage, ttl }
    }

    /// Create with default 30-day TTL
    pub fn with_defaults(storage: RedisStorage) -> Self {
        Self::new(storage, Duration::from_secs(86400 * 30))
    }

    /// Build the Redis key for a (index_uid, url) pair
    fn history_key(index_uid: &str, url: &str) -> String {
        format!("crawl_history:{}:{}", index_uid, url_hash(url))
    }

    /// Look up the crawl history for a URL in a given index.
    /// Returns None if the URL has never been crawled.
    pub async fn get(&self, index_uid: &str, url: &str) -> Result<Option<CrawlHistoryRecord>> {
        let key = Self::history_key(index_uid, url);
        match self.storage.get(&key).await? {
            Some(json) => {
                let record: CrawlHistoryRecord = serde_json::from_str(&json).map_err(|e| {
                    ScrapixError::Storage(format!("Failed to deserialize crawl history: {}", e))
                })?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    /// Save the crawl history for a URL after a successful fetch.
    pub async fn save(
        &self,
        index_uid: &str,
        url: &str,
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> Result<()> {
        let key = Self::history_key(index_uid, url);
        let record = CrawlHistoryRecord {
            etag,
            last_modified,
            crawled_at: chrono::Utc::now().timestamp_millis(),
        };
        let json = serde_json::to_string(&record).map_err(|e| {
            ScrapixError::Storage(format!("Failed to serialize crawl history: {}", e))
        })?;
        self.storage.set(&key, &json, Some(self.ttl)).await
    }
}

/// Hash a URL for use as a Redis key
fn url_hash(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redis_config_default() {
        let config = RedisConfig::default();
        assert_eq!(config.url, "redis://127.0.0.1:6379");
        assert_eq!(config.key_prefix, "scrapix:");
    }

    #[test]
    fn test_rate_limiter_config_default() {
        let config = RateLimiterConfig::default();
        assert_eq!(config.default_rate, 1.0);
        assert_eq!(config.burst_capacity, 10);
    }

    #[test]
    fn test_url_hash() {
        let hash1 = url_hash("https://example.com/page1");
        let hash2 = url_hash("https://example.com/page2");
        let hash3 = url_hash("https://example.com/page1");

        assert_ne!(hash1, hash2);
        assert_eq!(hash1, hash3);
    }
}
