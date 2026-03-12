//! Robots.txt parsing and caching
//!
//! This module provides both in-memory and persistent (RocksDB-backed) caching
//! for robots.txt files. The persistent cache survives restarts and can be shared
//! across worker instances.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use reqwest::Client;
use robotstxt::DefaultMatcher;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};
use url::Url;

use scrapix_core::{Result, ScrapixError};

/// Configuration for robots.txt handling
#[derive(Debug, Clone)]
pub struct RobotsConfig {
    /// User agent to match in robots.txt
    pub user_agent: String,
    /// Cache TTL for robots.txt files
    pub cache_ttl: Duration,
    /// Timeout for fetching robots.txt
    pub fetch_timeout: Duration,
    /// Whether to respect robots.txt (can be disabled for testing)
    pub respect_robots: bool,
    /// Default crawl delay if not specified
    pub default_crawl_delay_ms: Option<u64>,
}

impl Default for RobotsConfig {
    fn default() -> Self {
        Self {
            user_agent: "Scrapix".to_string(),
            cache_ttl: Duration::from_secs(3600), // 1 hour
            fetch_timeout: Duration::from_secs(10),
            respect_robots: true,
            default_crawl_delay_ms: None,
        }
    }
}

/// Cached robots.txt entry
struct CachedRobots {
    /// Raw robots.txt content
    content: String,
    /// When this entry was cached
    cached_at: Instant,
    /// Crawl delay from robots.txt (in milliseconds)
    crawl_delay_ms: Option<u64>,
}

/// Cache for robots.txt files
pub struct RobotsCache {
    config: RobotsConfig,
    client: Client,
    cache: RwLock<HashMap<String, CachedRobots>>,
}

impl RobotsCache {
    /// Create a new robots.txt cache
    pub fn new(config: RobotsConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.fetch_timeout)
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| ScrapixError::Crawl(format!("Failed to build robots client: {}", e)))?;

        Ok(Self {
            config,
            client,
            cache: RwLock::new(HashMap::new()),
        })
    }

    /// Create a new robots.txt cache with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(RobotsConfig::default())
    }

    /// Check if a URL is allowed by robots.txt
    #[instrument(skip(self))]
    pub async fn is_allowed(&self, url: &str) -> Result<bool> {
        if !self.config.respect_robots {
            return Ok(true);
        }

        let parsed = Url::parse(url)?;
        let domain = parsed
            .host_str()
            .ok_or_else(|| ScrapixError::Crawl("URL has no host".to_string()))?;

        let robots_content = self.get_robots(domain, &parsed).await?;
        let path = parsed.path();

        // Use robotstxt crate to check if allowed
        let mut matcher = DefaultMatcher::default();
        let allowed =
            matcher.one_agent_allowed_by_robots(&robots_content, &self.config.user_agent, path);

        debug!(url, allowed, "Robots.txt check");
        Ok(allowed)
    }

    /// Get crawl delay for a domain
    pub async fn get_crawl_delay(&self, domain: &str) -> Result<Option<u64>> {
        // Check cache first
        {
            let cache = self.cache.read();
            if let Some(entry) = cache.get(domain) {
                if entry.cached_at.elapsed() < self.config.cache_ttl {
                    return Ok(entry.crawl_delay_ms.or(self.config.default_crawl_delay_ms));
                }
            }
        }

        // We need a full URL to fetch robots.txt
        let robots_url = format!("https://{}/robots.txt", domain);
        let parsed = Url::parse(&robots_url)?;
        let _ = self.get_robots(domain, &parsed).await?;

        // Now check cache again
        let cache = self.cache.read();
        if let Some(entry) = cache.get(domain) {
            return Ok(entry.crawl_delay_ms.or(self.config.default_crawl_delay_ms));
        }

        Ok(self.config.default_crawl_delay_ms)
    }

    /// Get robots.txt content for a domain, fetching if needed
    async fn get_robots(&self, domain: &str, url: &Url) -> Result<String> {
        // Check cache first
        {
            let cache = self.cache.read();
            if let Some(entry) = cache.get(domain) {
                if entry.cached_at.elapsed() < self.config.cache_ttl {
                    return Ok(entry.content.clone());
                }
            }
        }

        // Fetch robots.txt
        let content = self.fetch_robots(url).await?;

        // Parse crawl delay
        let crawl_delay_ms = self.parse_crawl_delay(&content);

        // Cache the result
        {
            let mut cache = self.cache.write();
            cache.insert(
                domain.to_string(),
                CachedRobots {
                    content: content.clone(),
                    cached_at: Instant::now(),
                    crawl_delay_ms,
                },
            );
        }

        Ok(content)
    }

    /// Fetch robots.txt from a URL
    async fn fetch_robots(&self, url: &Url) -> Result<String> {
        let robots_url = format!(
            "{}://{}/robots.txt",
            url.scheme(),
            url.host_str().unwrap_or("")
        );

        debug!(robots_url, "Fetching robots.txt");

        match self.client.get(&robots_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let content = response.text().await.unwrap_or_else(|_| String::new());
                    Ok(content)
                } else if response.status().as_u16() == 404 {
                    // No robots.txt means everything is allowed
                    debug!(robots_url, "No robots.txt found (404)");
                    Ok(String::new())
                } else {
                    warn!(
                        robots_url,
                        status = response.status().as_u16(),
                        "Failed to fetch robots.txt"
                    );
                    // On error, be permissive
                    Ok(String::new())
                }
            }
            Err(e) => {
                warn!(robots_url, error = %e, "Error fetching robots.txt");
                // On network error, be permissive
                Ok(String::new())
            }
        }
    }

    /// Parse crawl delay from robots.txt content
    fn parse_crawl_delay(&self, content: &str) -> Option<u64> {
        let mut in_user_agent_section = false;
        let user_agent_lower = self.config.user_agent.to_lowercase();

        for line in content.lines() {
            let line = line.trim().to_lowercase();

            if line.starts_with("user-agent:") {
                let agent = line.trim_start_matches("user-agent:").trim();
                in_user_agent_section = agent == "*" || agent == user_agent_lower;
            } else if in_user_agent_section && line.starts_with("crawl-delay:") {
                let delay_str = line.trim_start_matches("crawl-delay:").trim();
                if let Ok(delay) = delay_str.parse::<f64>() {
                    if delay < 0.0 || delay.is_nan() || delay.is_infinite() {
                        return None;
                    }
                    // Convert seconds to milliseconds
                    return Some((delay * 1000.0) as u64);
                }
            }
        }

        None
    }

    /// Clear the cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write();
        cache.clear();
    }

    /// Get cache stats
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.cache.read();
        let total = cache.len();
        let expired = cache
            .values()
            .filter(|e| e.cached_at.elapsed() >= self.config.cache_ttl)
            .count();
        (total, total - expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_robots_config_default() {
        let config = RobotsConfig::default();
        assert_eq!(config.cache_ttl, Duration::from_secs(3600));
        assert!(config.respect_robots);
    }

    #[test]
    fn test_parse_crawl_delay() {
        let cache = RobotsCache::new(RobotsConfig::default()).unwrap();

        // Standard crawl delay
        let content = "User-agent: *\nCrawl-delay: 2";
        assert_eq!(cache.parse_crawl_delay(content), Some(2000));

        // Fractional delay
        let content = "User-agent: *\nCrawl-delay: 0.5";
        assert_eq!(cache.parse_crawl_delay(content), Some(500));

        // No crawl delay
        let content = "User-agent: *\nDisallow: /admin/";
        assert_eq!(cache.parse_crawl_delay(content), None);

        // Specific user agent
        let config = RobotsConfig {
            user_agent: "Scrapix".to_string(),
            ..Default::default()
        };
        let cache = RobotsCache::new(config).unwrap();
        let content = "User-agent: Scrapix\nCrawl-delay: 5\nUser-agent: *\nCrawl-delay: 1";
        assert_eq!(cache.parse_crawl_delay(content), Some(5000));
    }

    #[test]
    fn test_negative_crawl_delay() {
        let cache = RobotsCache::new(RobotsConfig::default()).unwrap();
        let content = "User-agent: *\nCrawl-delay: -5";
        assert_eq!(cache.parse_crawl_delay(content), None);
    }

    #[test]
    fn test_nan_crawl_delay() {
        let cache = RobotsCache::new(RobotsConfig::default()).unwrap();
        let content = "User-agent: *\nCrawl-delay: NaN";
        assert_eq!(cache.parse_crawl_delay(content), None);
    }

    #[test]
    fn test_infinity_crawl_delay() {
        let cache = RobotsCache::new(RobotsConfig::default()).unwrap();
        let content = "User-agent: *\nCrawl-delay: inf";
        assert_eq!(cache.parse_crawl_delay(content), None);
    }

    #[test]
    fn test_zero_crawl_delay() {
        let cache = RobotsCache::new(RobotsConfig::default()).unwrap();
        let content = "User-agent: *\nCrawl-delay: 0";
        assert_eq!(cache.parse_crawl_delay(content), Some(0));
    }

    #[test]
    fn test_fractional_crawl_delay() {
        let cache = RobotsCache::new(RobotsConfig::default()).unwrap();
        let content = "User-agent: *\nCrawl-delay: 0.5";
        assert_eq!(cache.parse_crawl_delay(content), Some(500));
    }
}

// ============================================================================
// Persistent Robots Cache (RocksDB-backed)
// ============================================================================

/// Serializable robots.txt cache entry for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentRobotsEntry {
    /// Raw robots.txt content
    pub content: String,
    /// Unix timestamp when cached (seconds since epoch)
    pub cached_at_unix: u64,
    /// Crawl delay in milliseconds
    pub crawl_delay_ms: Option<u64>,
    /// Sitemap URLs discovered from robots.txt
    pub sitemaps: Vec<String>,
}

impl PersistentRobotsEntry {
    /// Check if this entry has expired
    pub fn is_expired(&self, ttl: Duration) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.cached_at_unix) > ttl.as_secs()
    }

    /// Get age of this entry
    pub fn age(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Duration::from_secs(now.saturating_sub(self.cached_at_unix))
    }
}

/// Trait for robots.txt persistence storage
pub trait RobotsPersistence: Send + Sync {
    /// Get a cached entry by domain
    fn get(&self, domain: &str) -> Result<Option<PersistentRobotsEntry>>;

    /// Store a cache entry for a domain
    fn put(&self, domain: &str, entry: &PersistentRobotsEntry) -> Result<()>;

    /// Delete a cached entry
    fn delete(&self, domain: &str) -> Result<()>;

    /// Get all cached domains
    fn list_domains(&self) -> Result<Vec<String>>;

    /// Count cached entries
    fn count(&self) -> Result<usize>;

    /// Clear all cached entries
    fn clear(&self) -> Result<()>;
}

/// RocksDB-backed robots.txt persistence
pub struct RocksRobotsPersistence {
    storage: Arc<dyn RocksDbOps>,
    column_family: String,
}

/// Trait abstracting RocksDB operations for testability
pub trait RocksDbOps: Send + Sync {
    fn get_cf(&self, cf: &str, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put_cf(&self, cf: &str, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete_cf(&self, cf: &str, key: &[u8]) -> Result<()>;
    fn prefix_iter_cf(&self, cf: &str, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
}

impl RocksRobotsPersistence {
    /// Create a new RocksDB robots persistence with a storage backend
    pub fn new(storage: Arc<dyn RocksDbOps>, column_family: impl Into<String>) -> Self {
        Self {
            storage,
            column_family: column_family.into(),
        }
    }

    /// Create with default column family name
    pub fn with_defaults(storage: Arc<dyn RocksDbOps>) -> Self {
        Self::new(storage, "robots_cache")
    }

    fn domain_key(domain: &str) -> Vec<u8> {
        format!("robots:{}", domain).into_bytes()
    }
}

impl RobotsPersistence for RocksRobotsPersistence {
    fn get(&self, domain: &str) -> Result<Option<PersistentRobotsEntry>> {
        let key = Self::domain_key(domain);
        match self.storage.get_cf(&self.column_family, &key)? {
            Some(data) => {
                let entry: PersistentRobotsEntry = serde_json::from_slice(&data).map_err(|e| {
                    ScrapixError::Storage(format!("Failed to deserialize robots entry: {}", e))
                })?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    fn put(&self, domain: &str, entry: &PersistentRobotsEntry) -> Result<()> {
        let key = Self::domain_key(domain);
        let data = serde_json::to_vec(entry).map_err(|e| {
            ScrapixError::Storage(format!("Failed to serialize robots entry: {}", e))
        })?;
        self.storage.put_cf(&self.column_family, &key, &data)
    }

    fn delete(&self, domain: &str) -> Result<()> {
        let key = Self::domain_key(domain);
        self.storage.delete_cf(&self.column_family, &key)
    }

    fn list_domains(&self) -> Result<Vec<String>> {
        let prefix = b"robots:";
        let entries = self.storage.prefix_iter_cf(&self.column_family, prefix)?;

        let domains: Vec<String> = entries
            .into_iter()
            .filter_map(|(key, _)| {
                String::from_utf8(key)
                    .ok()
                    .and_then(|s| s.strip_prefix("robots:").map(|d| d.to_string()))
            })
            .collect();

        Ok(domains)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.list_domains()?.len())
    }

    fn clear(&self) -> Result<()> {
        for domain in self.list_domains()? {
            self.delete(&domain)?;
        }
        Ok(())
    }
}

/// Persistent robots.txt cache backed by RocksDB
///
/// This cache provides:
/// - Fast in-memory lookups with L1 cache
/// - Persistent storage in RocksDB for L2 cache
/// - Automatic expiration based on TTL
/// - Sitemap discovery from robots.txt
pub struct PersistentRobotsCache {
    config: RobotsConfig,
    client: Client,
    /// L1 in-memory cache for hot entries
    memory_cache: RwLock<HashMap<String, CachedRobotsMemory>>,
    /// L2 persistent storage
    persistence: Arc<dyn RobotsPersistence>,
}

/// In-memory cache entry (hot cache)
struct CachedRobotsMemory {
    content: String,
    cached_at: Instant,
    crawl_delay_ms: Option<u64>,
    sitemaps: Vec<String>,
}

impl PersistentRobotsCache {
    /// Create a new persistent robots cache
    pub fn new(config: RobotsConfig, persistence: Arc<dyn RobotsPersistence>) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.fetch_timeout)
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| ScrapixError::Crawl(format!("Failed to build robots client: {}", e)))?;

        Ok(Self {
            config,
            client,
            memory_cache: RwLock::new(HashMap::new()),
            persistence,
        })
    }

    /// Check if a URL is allowed by robots.txt
    #[instrument(skip(self))]
    pub async fn is_allowed(&self, url: &str) -> Result<bool> {
        if !self.config.respect_robots {
            return Ok(true);
        }

        let parsed = Url::parse(url)?;
        let domain = parsed
            .host_str()
            .ok_or_else(|| ScrapixError::Crawl("URL has no host".to_string()))?;

        let robots_content = self.get_robots(domain, &parsed).await?;
        let path = parsed.path();

        let mut matcher = DefaultMatcher::default();
        let allowed =
            matcher.one_agent_allowed_by_robots(&robots_content, &self.config.user_agent, path);

        debug!(url, allowed, "Robots.txt check");
        Ok(allowed)
    }

    /// Get crawl delay for a domain
    pub async fn get_crawl_delay(&self, domain: &str) -> Result<Option<u64>> {
        // Check L1 memory cache first
        {
            let cache = self.memory_cache.read();
            if let Some(entry) = cache.get(domain) {
                if entry.cached_at.elapsed() < self.config.cache_ttl {
                    return Ok(entry.crawl_delay_ms.or(self.config.default_crawl_delay_ms));
                }
            }
        }

        // Check L2 persistent cache
        if let Some(entry) = self.persistence.get(domain)? {
            if !entry.is_expired(self.config.cache_ttl) {
                // Promote to L1 cache
                self.update_memory_cache(domain, &entry);
                return Ok(entry.crawl_delay_ms.or(self.config.default_crawl_delay_ms));
            }
        }

        // Fetch fresh
        let robots_url = format!("https://{}/robots.txt", domain);
        let parsed = Url::parse(&robots_url)?;
        let _ = self.get_robots(domain, &parsed).await?;

        // Now check L1 cache
        let cache = self.memory_cache.read();
        if let Some(entry) = cache.get(domain) {
            return Ok(entry.crawl_delay_ms.or(self.config.default_crawl_delay_ms));
        }

        Ok(self.config.default_crawl_delay_ms)
    }

    /// Get sitemap URLs discovered from robots.txt
    pub async fn get_sitemaps(&self, domain: &str) -> Result<Vec<String>> {
        // Check L1 memory cache first
        {
            let cache = self.memory_cache.read();
            if let Some(entry) = cache.get(domain) {
                if entry.cached_at.elapsed() < self.config.cache_ttl {
                    return Ok(entry.sitemaps.clone());
                }
            }
        }

        // Check L2 persistent cache
        if let Some(entry) = self.persistence.get(domain)? {
            if !entry.is_expired(self.config.cache_ttl) {
                self.update_memory_cache(domain, &entry);
                return Ok(entry.sitemaps.clone());
            }
        }

        // Fetch fresh
        let robots_url = format!("https://{}/robots.txt", domain);
        let parsed = Url::parse(&robots_url)?;
        let _ = self.get_robots(domain, &parsed).await?;

        let cache = self.memory_cache.read();
        if let Some(entry) = cache.get(domain) {
            return Ok(entry.sitemaps.clone());
        }

        Ok(vec![])
    }

    /// Get robots.txt content for a domain
    async fn get_robots(&self, domain: &str, url: &Url) -> Result<String> {
        // Check L1 memory cache
        {
            let cache = self.memory_cache.read();
            if let Some(entry) = cache.get(domain) {
                if entry.cached_at.elapsed() < self.config.cache_ttl {
                    return Ok(entry.content.clone());
                }
            }
        }

        // Check L2 persistent cache
        if let Some(entry) = self.persistence.get(domain)? {
            if !entry.is_expired(self.config.cache_ttl) {
                self.update_memory_cache(domain, &entry);
                return Ok(entry.content.clone());
            }
        }

        // Fetch fresh
        let content = self.fetch_robots(url).await?;
        let crawl_delay_ms = self.parse_crawl_delay(&content);
        let sitemaps = self.parse_sitemaps(&content);

        // Store in both caches
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let persistent_entry = PersistentRobotsEntry {
            content: content.clone(),
            cached_at_unix: now_unix,
            crawl_delay_ms,
            sitemaps: sitemaps.clone(),
        };

        // Store in L2 (persistent)
        if let Err(e) = self.persistence.put(domain, &persistent_entry) {
            warn!(domain, error = %e, "Failed to persist robots.txt cache");
        }

        // Store in L1 (memory)
        {
            let mut cache = self.memory_cache.write();
            cache.insert(
                domain.to_string(),
                CachedRobotsMemory {
                    content: content.clone(),
                    cached_at: Instant::now(),
                    crawl_delay_ms,
                    sitemaps,
                },
            );
        }

        Ok(content)
    }

    /// Fetch robots.txt from URL
    async fn fetch_robots(&self, url: &Url) -> Result<String> {
        let robots_url = format!(
            "{}://{}/robots.txt",
            url.scheme(),
            url.host_str().unwrap_or("")
        );

        debug!(robots_url, "Fetching robots.txt");

        match self.client.get(&robots_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(response.text().await.unwrap_or_default())
                } else if response.status().as_u16() == 404 {
                    debug!(robots_url, "No robots.txt found (404)");
                    Ok(String::new())
                } else {
                    warn!(
                        robots_url,
                        status = response.status().as_u16(),
                        "Failed to fetch robots.txt"
                    );
                    Ok(String::new())
                }
            }
            Err(e) => {
                warn!(robots_url, error = %e, "Error fetching robots.txt");
                Ok(String::new())
            }
        }
    }

    /// Parse crawl delay from robots.txt
    fn parse_crawl_delay(&self, content: &str) -> Option<u64> {
        let mut in_user_agent_section = false;
        let user_agent_lower = self.config.user_agent.to_lowercase();

        for line in content.lines() {
            let line = line.trim().to_lowercase();

            if line.starts_with("user-agent:") {
                let agent = line.trim_start_matches("user-agent:").trim();
                in_user_agent_section = agent == "*" || agent == user_agent_lower;
            } else if in_user_agent_section && line.starts_with("crawl-delay:") {
                let delay_str = line.trim_start_matches("crawl-delay:").trim();
                if let Ok(delay) = delay_str.parse::<f64>() {
                    if delay < 0.0 || delay.is_nan() || delay.is_infinite() {
                        return None;
                    }
                    return Some((delay * 1000.0) as u64);
                }
            }
        }

        None
    }

    /// Parse sitemap URLs from robots.txt
    fn parse_sitemaps(&self, content: &str) -> Vec<String> {
        content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.to_lowercase().starts_with("sitemap:") {
                    Some(line[8..].trim().to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Update L1 memory cache from persistent entry
    fn update_memory_cache(&self, domain: &str, entry: &PersistentRobotsEntry) {
        let mut cache = self.memory_cache.write();
        cache.insert(
            domain.to_string(),
            CachedRobotsMemory {
                content: entry.content.clone(),
                cached_at: Instant::now(),
                crawl_delay_ms: entry.crawl_delay_ms,
                sitemaps: entry.sitemaps.clone(),
            },
        );
    }

    /// Clear all caches
    pub fn clear_cache(&self) -> Result<()> {
        {
            let mut cache = self.memory_cache.write();
            cache.clear();
        }
        self.persistence.clear()
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> Result<RobotsCacheStats> {
        let memory_cache = self.memory_cache.read();
        let memory_total = memory_cache.len();
        let memory_valid = memory_cache
            .values()
            .filter(|e| e.cached_at.elapsed() < self.config.cache_ttl)
            .count();

        let persistent_total = self.persistence.count()?;

        Ok(RobotsCacheStats {
            memory_total,
            memory_valid,
            persistent_total,
        })
    }

    /// Prefetch robots.txt for multiple domains
    pub async fn prefetch(&self, domains: &[String]) -> Result<()> {
        for domain in domains {
            let robots_url = format!("https://{}/robots.txt", domain);
            if let Ok(parsed) = Url::parse(&robots_url) {
                let _ = self.get_robots(domain, &parsed).await;
            }
        }
        Ok(())
    }

    /// Load all persistent entries into memory cache
    pub fn warm_memory_cache(&self) -> Result<usize> {
        let domains = self.persistence.list_domains()?;
        let mut loaded = 0;

        for domain in &domains {
            if let Some(entry) = self.persistence.get(domain)? {
                if !entry.is_expired(self.config.cache_ttl) {
                    self.update_memory_cache(domain, &entry);
                    loaded += 1;
                }
            }
        }

        Ok(loaded)
    }
}

/// Statistics about the robots.txt cache
#[derive(Debug, Clone)]
pub struct RobotsCacheStats {
    /// Total entries in memory cache
    pub memory_total: usize,
    /// Valid (non-expired) entries in memory cache
    pub memory_valid: usize,
    /// Total entries in persistent cache
    pub persistent_total: usize,
}

#[cfg(test)]
mod persistent_tests {
    use super::*;

    /// In-memory implementation for testing
    struct MemoryRobotsPersistence {
        data: RwLock<HashMap<String, PersistentRobotsEntry>>,
    }

    impl MemoryRobotsPersistence {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    impl RobotsPersistence for MemoryRobotsPersistence {
        fn get(&self, domain: &str) -> Result<Option<PersistentRobotsEntry>> {
            Ok(self.data.read().get(domain).cloned())
        }

        fn put(&self, domain: &str, entry: &PersistentRobotsEntry) -> Result<()> {
            self.data.write().insert(domain.to_string(), entry.clone());
            Ok(())
        }

        fn delete(&self, domain: &str) -> Result<()> {
            self.data.write().remove(domain);
            Ok(())
        }

        fn list_domains(&self) -> Result<Vec<String>> {
            Ok(self.data.read().keys().cloned().collect())
        }

        fn count(&self) -> Result<usize> {
            Ok(self.data.read().len())
        }

        fn clear(&self) -> Result<()> {
            self.data.write().clear();
            Ok(())
        }
    }

    #[test]
    fn test_persistent_entry_expiration() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = PersistentRobotsEntry {
            content: "User-agent: *\nDisallow:".to_string(),
            cached_at_unix: now,
            crawl_delay_ms: None,
            sitemaps: vec![],
        };

        // Fresh entry should not be expired
        assert!(!entry.is_expired(Duration::from_secs(3600)));

        // Old entry should be expired
        let old_entry = PersistentRobotsEntry {
            cached_at_unix: now - 7200, // 2 hours ago
            ..entry.clone()
        };
        assert!(old_entry.is_expired(Duration::from_secs(3600)));
    }

    #[test]
    fn test_parse_sitemaps() {
        let config = RobotsConfig::default();
        let persistence = Arc::new(MemoryRobotsPersistence::new());
        let cache = PersistentRobotsCache::new(config, persistence).unwrap();

        let content = r#"
User-agent: *
Disallow: /admin/

Sitemap: https://example.com/sitemap.xml
Sitemap: https://example.com/sitemap-news.xml
"#;

        let sitemaps = cache.parse_sitemaps(content);
        assert_eq!(sitemaps.len(), 2);
        assert!(sitemaps.contains(&"https://example.com/sitemap.xml".to_string()));
        assert!(sitemaps.contains(&"https://example.com/sitemap-news.xml".to_string()));
    }

    #[test]
    fn test_memory_persistence() {
        let persistence = MemoryRobotsPersistence::new();

        let entry = PersistentRobotsEntry {
            content: "User-agent: *".to_string(),
            cached_at_unix: 12345,
            crawl_delay_ms: Some(1000),
            sitemaps: vec!["https://example.com/sitemap.xml".to_string()],
        };

        // Put and get
        persistence.put("example.com", &entry).unwrap();
        let retrieved = persistence.get("example.com").unwrap().unwrap();
        assert_eq!(retrieved.content, entry.content);
        assert_eq!(retrieved.crawl_delay_ms, entry.crawl_delay_ms);

        // List domains
        persistence.put("other.com", &entry).unwrap();
        let domains = persistence.list_domains().unwrap();
        assert_eq!(domains.len(), 2);

        // Count
        assert_eq!(persistence.count().unwrap(), 2);

        // Delete
        persistence.delete("example.com").unwrap();
        assert_eq!(persistence.count().unwrap(), 1);

        // Clear
        persistence.clear().unwrap();
        assert_eq!(persistence.count().unwrap(), 0);
    }
}
