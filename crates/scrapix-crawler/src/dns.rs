//! DNS resolution with caching

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use hickory_resolver::config::{ResolveHosts, ResolverConfig};
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::Resolver;
use parking_lot::RwLock;
use tracing::{debug, instrument};

/// Type alias for the Tokio-based resolver
pub type TokioResolver = Resolver<TokioConnectionProvider>;

use scrapix_core::{Result, ScrapixError};

/// Configuration for DNS resolver
#[derive(Debug, Clone)]
pub struct DnsConfig {
    /// Cache TTL for DNS entries (positive)
    pub cache_ttl: Duration,
    /// Cache TTL for negative (NXDOMAIN) entries
    pub negative_cache_ttl: Duration,
    /// Maximum cache size
    pub max_cache_size: usize,
    /// Whether to use system DNS or Google DNS
    pub use_system_dns: bool,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(300),         // 5 minutes
            negative_cache_ttl: Duration::from_secs(60), // 1 minute
            max_cache_size: 10_000,
            use_system_dns: true,
        }
    }
}

/// Cached DNS entry
struct CachedDns {
    /// Resolved IP addresses
    addresses: Vec<IpAddr>,
    /// When this entry was cached
    cached_at: Instant,
    /// Whether this is a negative (failed) entry
    is_negative: bool,
}

/// DNS resolver with caching
pub struct CachingDnsResolver {
    config: DnsConfig,
    resolver: TokioResolver,
    cache: RwLock<HashMap<String, CachedDns>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl CachingDnsResolver {
    /// Create a new DNS resolver
    pub fn new(config: DnsConfig) -> Result<Self> {
        let resolver_config = if config.use_system_dns {
            ResolverConfig::default()
        } else {
            ResolverConfig::google()
        };

        let mut builder =
            Resolver::builder_with_config(resolver_config, TokioConnectionProvider::default());

        // Configure resolver options
        let opts = builder.options_mut();
        opts.cache_size = 0; // We handle caching ourselves
        opts.use_hosts_file = ResolveHosts::Always;
        opts.positive_min_ttl = Some(config.cache_ttl);
        opts.negative_min_ttl = Some(config.negative_cache_ttl);

        let resolver = builder.build();

        Ok(Self {
            config,
            resolver,
            cache: RwLock::new(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        })
    }

    /// Create a new DNS resolver with default configuration
    pub fn with_defaults() -> Result<Self> {
        Self::new(DnsConfig::default())
    }

    /// Resolve a hostname to IP addresses
    #[instrument(skip(self))]
    pub async fn resolve(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        // Check cache first
        {
            let cache = self.cache.read();
            if let Some(entry) = cache.get(hostname) {
                let ttl = if entry.is_negative {
                    self.config.negative_cache_ttl
                } else {
                    self.config.cache_ttl
                };

                if entry.cached_at.elapsed() < ttl {
                    self.hits.fetch_add(1, Ordering::Relaxed);
                    if entry.is_negative {
                        return Err(ScrapixError::Crawl(format!(
                            "DNS resolution failed for {} (cached)",
                            hostname
                        )));
                    }
                    debug!(hostname, "DNS cache hit");
                    return Ok(entry.addresses.clone());
                }
            }
        }

        self.misses.fetch_add(1, Ordering::Relaxed);

        // Resolve
        match self.resolver.lookup_ip(hostname).await {
            Ok(response) => {
                let addresses: Vec<IpAddr> = response.iter().collect();

                // Cache the result
                self.cache_result(hostname, addresses.clone(), false);

                debug!(hostname, count = addresses.len(), "DNS resolved");
                Ok(addresses)
            }
            Err(e) => {
                // Cache negative result
                self.cache_result(hostname, vec![], true);

                Err(ScrapixError::Crawl(format!(
                    "DNS resolution failed for {}: {}",
                    hostname, e
                )))
            }
        }
    }

    /// Cache a DNS result
    fn cache_result(&self, hostname: &str, addresses: Vec<IpAddr>, is_negative: bool) {
        let mut cache = self.cache.write();

        // Evict if cache is full
        if cache.len() >= self.config.max_cache_size {
            // Simple eviction: remove oldest entries
            cache.retain(|_, entry| {
                let ttl = if entry.is_negative {
                    self.config.negative_cache_ttl
                } else {
                    self.config.cache_ttl
                };
                entry.cached_at.elapsed() < ttl
            });

            // If still too large, remove half
            if cache.len() >= self.config.max_cache_size {
                let mut entries: Vec<_> = cache.iter().collect();
                entries.sort_by(|a, b| a.1.cached_at.cmp(&b.1.cached_at));
                let to_remove: Vec<_> = entries
                    .iter()
                    .take(cache.len() / 2)
                    .map(|(k, _)| (*k).clone())
                    .collect();
                for key in to_remove {
                    cache.remove(&key);
                }
            }
        }

        cache.insert(
            hostname.to_string(),
            CachedDns {
                addresses,
                cached_at: Instant::now(),
                is_negative,
            },
        );
    }

    /// Clear the DNS cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write();
        cache.clear();
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }

    /// Get cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;

        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get cache stats
    pub fn cache_stats(&self) -> DnsCacheStats {
        let cache = self.cache.read();
        DnsCacheStats {
            size: cache.len(),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            hit_rate: self.cache_hit_rate(),
        }
    }
}

/// DNS cache statistics
#[derive(Debug, Clone)]
pub struct DnsCacheStats {
    pub size: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
}

/// Trait implementation for core DnsResolver trait
#[async_trait]
impl scrapix_core::traits::DnsResolver for CachingDnsResolver {
    async fn resolve(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        CachingDnsResolver::resolve(self, hostname).await
    }

    async fn clear_cache(&self) -> Result<()> {
        CachingDnsResolver::clear_cache(self);
        Ok(())
    }

    fn cache_hit_rate(&self) -> f64 {
        CachingDnsResolver::cache_hit_rate(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_config_default() {
        let config = DnsConfig::default();
        assert_eq!(config.cache_ttl, Duration::from_secs(300));
        assert!(config.use_system_dns);
    }

    #[tokio::test]
    async fn test_resolve_localhost() {
        let resolver = CachingDnsResolver::with_defaults().unwrap();
        let result = resolver.resolve("localhost").await;
        assert!(result.is_ok());
        let addresses = result.unwrap();
        assert!(!addresses.is_empty());
    }

    #[tokio::test]
    async fn test_cache_hit() {
        let resolver = CachingDnsResolver::with_defaults().unwrap();

        // First resolution (miss)
        let _ = resolver.resolve("localhost").await;
        assert_eq!(resolver.cache_stats().misses, 1);
        assert_eq!(resolver.cache_stats().hits, 0);

        // Second resolution (hit)
        let _ = resolver.resolve("localhost").await;
        assert_eq!(resolver.cache_stats().hits, 1);
    }
}
