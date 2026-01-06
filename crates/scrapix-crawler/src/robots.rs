//! Robots.txt parsing and caching

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use reqwest::Client;
use robotstxt::DefaultMatcher;
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
}
