//! Proxy management and rotation

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use reqwest::Proxy;
use tracing::{debug, warn};

use scrapix_core::{Result, ScrapixError};

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// List of proxy URLs
    pub proxies: Vec<String>,
    /// Rotation strategy
    pub rotation: RotationStrategy,
    /// Retry failed proxies after this duration
    pub failure_cooldown: Duration,
    /// Maximum consecutive failures before removing proxy
    pub max_failures: u32,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            proxies: vec![],
            rotation: RotationStrategy::RoundRobin,
            failure_cooldown: Duration::from_secs(300),
            max_failures: 5,
        }
    }
}

/// Proxy rotation strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationStrategy {
    /// Cycle through proxies in order
    RoundRobin,
    /// Pick a random proxy
    Random,
    /// Use the proxy with least recent use
    LeastRecentlyUsed,
}

/// Internal proxy state
struct ProxyState {
    /// Proxy URL
    url: String,
    /// Number of consecutive failures
    failures: u32,
    /// Last failure time
    last_failure: Option<Instant>,
    /// Last use time
    last_used: Option<Instant>,
    /// Total requests made through this proxy
    total_requests: u64,
    /// Total successful requests
    successful_requests: u64,
}

impl ProxyState {
    fn new(url: String) -> Self {
        Self {
            url,
            failures: 0,
            last_failure: None,
            last_used: None,
            total_requests: 0,
            successful_requests: 0,
        }
    }

    fn is_available(&self, cooldown: Duration, max_failures: u32) -> bool {
        // No failures - always available
        if self.failures == 0 {
            return true;
        }

        // Below max failures threshold - still available
        if self.failures < max_failures {
            return true;
        }

        // At or above max failures - check if cooldown has elapsed
        match self.last_failure {
            Some(last) => last.elapsed() >= cooldown,
            None => true,
        }
    }

    fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            1.0
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }
}

/// Proxy pool with rotation and failure handling
pub struct ProxyPool {
    config: ProxyConfig,
    proxies: RwLock<Vec<ProxyState>>,
    current_index: AtomicUsize,
}

impl ProxyPool {
    /// Create a new proxy pool
    pub fn new(config: ProxyConfig) -> Self {
        let proxies: Vec<_> = config
            .proxies
            .iter()
            .map(|url| ProxyState::new(url.clone()))
            .collect();

        Self {
            config,
            proxies: RwLock::new(proxies),
            current_index: AtomicUsize::new(0),
        }
    }

    /// Create an empty proxy pool (direct connections)
    pub fn empty() -> Self {
        Self::new(ProxyConfig::default())
    }

    /// Check if the pool has any proxies
    pub fn is_empty(&self) -> bool {
        self.proxies.read().is_empty()
    }

    /// Get the number of available proxies
    pub fn available_count(&self) -> usize {
        let proxies = self.proxies.read();
        proxies
            .iter()
            .filter(|p| p.is_available(self.config.failure_cooldown, self.config.max_failures))
            .count()
    }

    /// Get the next proxy to use
    pub fn get_proxy(&self) -> Option<String> {
        let mut proxies = self.proxies.write();

        if proxies.is_empty() {
            return None;
        }

        let available: Vec<usize> = proxies
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_available(self.config.failure_cooldown, self.config.max_failures))
            .map(|(i, _)| i)
            .collect();

        if available.is_empty() {
            warn!("No available proxies, all in cooldown");
            return None;
        }

        let index = match self.config.rotation {
            RotationStrategy::RoundRobin => {
                let current = self.current_index.fetch_add(1, Ordering::Relaxed);
                available[current % available.len()]
            }
            RotationStrategy::Random => {
                use std::time::SystemTime;
                let seed = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as usize)
                    .unwrap_or(0);
                available[seed % available.len()]
            }
            RotationStrategy::LeastRecentlyUsed => {
                let mut lru_index = available[0];
                let mut lru_time = proxies[lru_index].last_used;

                for &i in &available[1..] {
                    let proxy_time = proxies[i].last_used;
                    if proxy_time.is_none() || (lru_time.is_some() && proxy_time < lru_time) {
                        lru_index = i;
                        lru_time = proxy_time;
                    }
                }
                lru_index
            }
        };

        proxies[index].last_used = Some(Instant::now());
        proxies[index].total_requests += 1;

        Some(proxies[index].url.clone())
    }

    /// Build a reqwest Proxy from the next available proxy
    pub fn get_reqwest_proxy(&self) -> Result<Option<Proxy>> {
        match self.get_proxy() {
            Some(url) => {
                let proxy = Proxy::all(&url)
                    .map_err(|e| ScrapixError::Crawl(format!("Invalid proxy URL: {}", e)))?;
                Ok(Some(proxy))
            }
            None => Ok(None),
        }
    }

    /// Report a successful request through a proxy
    pub fn report_success(&self, proxy_url: &str) {
        let mut proxies = self.proxies.write();
        if let Some(proxy) = proxies.iter_mut().find(|p| p.url == proxy_url) {
            proxy.failures = 0;
            proxy.last_failure = None;
            proxy.successful_requests += 1;
            debug!(proxy = %proxy_url, "Proxy request succeeded");
        }
    }

    /// Report a failed request through a proxy
    pub fn report_failure(&self, proxy_url: &str) {
        let mut proxies = self.proxies.write();
        if let Some(proxy) = proxies.iter_mut().find(|p| p.url == proxy_url) {
            proxy.failures += 1;
            proxy.last_failure = Some(Instant::now());

            warn!(
                proxy = %proxy_url,
                failures = proxy.failures,
                "Proxy request failed"
            );

            if proxy.failures >= self.config.max_failures {
                warn!(
                    proxy = %proxy_url,
                    "Proxy exceeded max failures, removing from pool"
                );
                // Mark for removal by setting very high failure count
                proxy.failures = u32::MAX;
            }
        }
    }

    /// Get proxy statistics
    pub fn stats(&self) -> Vec<ProxyStats> {
        let proxies = self.proxies.read();
        proxies
            .iter()
            .map(|p| ProxyStats {
                url: p.url.clone(),
                total_requests: p.total_requests,
                successful_requests: p.successful_requests,
                success_rate: p.success_rate(),
                consecutive_failures: p.failures,
                is_available: p.is_available(self.config.failure_cooldown, self.config.max_failures),
            })
            .collect()
    }

    /// Add a new proxy to the pool
    pub fn add_proxy(&self, url: String) {
        let mut proxies = self.proxies.write();
        if !proxies.iter().any(|p| p.url == url) {
            proxies.push(ProxyState::new(url));
        }
    }

    /// Remove a proxy from the pool
    pub fn remove_proxy(&self, url: &str) {
        let mut proxies = self.proxies.write();
        proxies.retain(|p| p.url != url);
    }

    /// Clean up failed proxies
    pub fn cleanup_failed(&self) {
        let mut proxies = self.proxies.write();
        proxies.retain(|p| p.failures < self.config.max_failures);
    }
}

/// Proxy statistics
#[derive(Debug, Clone)]
pub struct ProxyStats {
    pub url: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub success_rate: f64,
    pub consecutive_failures: u32,
    pub is_available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pool() {
        let pool = ProxyPool::empty();
        assert!(pool.is_empty());
        assert_eq!(pool.get_proxy(), None);
    }

    #[test]
    fn test_round_robin() {
        let config = ProxyConfig {
            proxies: vec![
                "http://proxy1:8080".to_string(),
                "http://proxy2:8080".to_string(),
                "http://proxy3:8080".to_string(),
            ],
            rotation: RotationStrategy::RoundRobin,
            ..Default::default()
        };

        let pool = ProxyPool::new(config);

        let first = pool.get_proxy();
        let second = pool.get_proxy();
        let third = pool.get_proxy();
        let fourth = pool.get_proxy();

        assert!(first.is_some());
        assert!(second.is_some());
        assert!(third.is_some());
        assert!(fourth.is_some());

        // Fourth should wrap around to first
        assert_eq!(first, fourth);
    }

    #[test]
    fn test_failure_cooldown() {
        let config = ProxyConfig {
            proxies: vec!["http://proxy1:8080".to_string()],
            failure_cooldown: Duration::from_millis(100),
            max_failures: 3,
            ..Default::default()
        };

        let pool = ProxyPool::new(config);

        // Report failure
        pool.report_failure("http://proxy1:8080");

        // Should still be available (1 failure < max_failures)
        assert!(pool.get_proxy().is_some());

        // Report more failures
        pool.report_failure("http://proxy1:8080");
        pool.report_failure("http://proxy1:8080");

        // Now should be in cooldown
        assert!(pool.get_proxy().is_none());
    }

    #[test]
    fn test_success_resets_failures() {
        let config = ProxyConfig {
            proxies: vec!["http://proxy1:8080".to_string()],
            max_failures: 5,
            ..Default::default()
        };

        let pool = ProxyPool::new(config);

        // Report some failures
        pool.report_failure("http://proxy1:8080");
        pool.report_failure("http://proxy1:8080");

        // Report success
        pool.report_success("http://proxy1:8080");

        // Check stats
        let stats = pool.stats();
        assert_eq!(stats[0].consecutive_failures, 0);
    }
}
