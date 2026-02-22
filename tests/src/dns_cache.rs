//! Integration tests for DNS caching functionality

use std::sync::Arc;
use std::time::Duration;

use scrapix_crawler::{
    CachingDnsResolver, DnsConfig, HttpFetcherBuilder, RobotsCache, RobotsConfig,
};

/// Test DNS resolver creation with default config
#[tokio::test]
async fn test_dns_resolver_default() {
    let resolver = CachingDnsResolver::with_defaults().unwrap();

    // Resolve localhost
    let result = resolver.resolve("localhost").await;
    assert!(result.is_ok());
    let addresses = result.unwrap();
    assert!(!addresses.is_empty());
}

/// Test DNS cache hit tracking
#[tokio::test]
async fn test_dns_cache_hit_tracking() {
    let resolver = CachingDnsResolver::with_defaults().unwrap();

    // First resolution should be a miss
    let _ = resolver.resolve("localhost").await;
    let stats = resolver.cache_stats();
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hits, 0);

    // Second resolution should be a hit
    let _ = resolver.resolve("localhost").await;
    let stats = resolver.cache_stats();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
    assert!(stats.hit_rate > 0.0);
}

/// Test DNS cache TTL configuration
#[tokio::test]
async fn test_dns_cache_ttl_config() {
    let config = DnsConfig {
        cache_ttl: Duration::from_secs(1),
        negative_cache_ttl: Duration::from_millis(500),
        max_cache_size: 100,
        use_system_dns: true,
    };

    let resolver = CachingDnsResolver::new(config).unwrap();

    // First resolution
    let _ = resolver.resolve("localhost").await;
    assert_eq!(resolver.cache_stats().misses, 1);

    // Should hit cache
    let _ = resolver.resolve("localhost").await;
    assert_eq!(resolver.cache_stats().hits, 1);

    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Should be a miss again
    let _ = resolver.resolve("localhost").await;
    assert_eq!(resolver.cache_stats().misses, 2);
}

/// Test DNS cache clearing
#[tokio::test]
async fn test_dns_cache_clear() {
    let resolver = CachingDnsResolver::with_defaults().unwrap();

    // Populate cache
    let _ = resolver.resolve("localhost").await;
    assert_eq!(resolver.cache_stats().size, 1);

    // Clear cache
    resolver.clear_cache();
    let stats = resolver.cache_stats();
    assert_eq!(stats.size, 0);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
}

/// Test negative caching (failed resolutions)
#[tokio::test]
async fn test_dns_negative_cache() {
    let config = DnsConfig {
        cache_ttl: Duration::from_secs(300),
        negative_cache_ttl: Duration::from_secs(60),
        max_cache_size: 1000,
        use_system_dns: true,
    };

    let resolver = CachingDnsResolver::new(config).unwrap();

    // Try to resolve a non-existent domain (should fail and be cached)
    let result = resolver
        .resolve("this-domain-definitely-does-not-exist-12345.invalid")
        .await;
    assert!(result.is_err());
    assert_eq!(resolver.cache_stats().misses, 1);

    // Second attempt should hit the negative cache
    let result = resolver
        .resolve("this-domain-definitely-does-not-exist-12345.invalid")
        .await;
    assert!(result.is_err());
    assert_eq!(resolver.cache_stats().hits, 1);
}

/// Test fetcher with DNS cache enabled
#[tokio::test]
async fn test_fetcher_with_dns_cache() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .with_dns_cache()
        .build(robots_cache)
        .unwrap();

    // Check DNS cache is enabled
    assert!(fetcher.has_dns_cache());

    // Get initial stats
    let stats = fetcher.dns_cache_stats().unwrap();
    assert_eq!(stats.size, 0);
}

/// Test fetcher with custom DNS config
#[tokio::test]
async fn test_fetcher_with_custom_dns_config() {
    let dns_config = DnsConfig {
        cache_ttl: Duration::from_secs(600),
        negative_cache_ttl: Duration::from_secs(120),
        max_cache_size: 5000,
        use_system_dns: true,
    };

    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .with_dns_config(dns_config)
        .build(robots_cache)
        .unwrap();

    assert!(fetcher.has_dns_cache());
}

/// Test fetcher without DNS cache
#[tokio::test]
async fn test_fetcher_without_dns_cache() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .build(robots_cache)
        .unwrap();

    // DNS cache should not be enabled by default
    assert!(!fetcher.has_dns_cache());
    assert!(fetcher.dns_cache_stats().is_none());
}

/// Test DNS pre-resolution via fetcher
#[tokio::test]
async fn test_fetcher_dns_pre_resolution() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .with_dns_cache()
        .build(robots_cache)
        .unwrap();

    // Pre-resolve DNS
    let result = fetcher.resolve_dns("localhost").await;
    assert!(result.is_ok());

    // Check cache was populated
    let stats = fetcher.dns_cache_stats().unwrap();
    assert_eq!(stats.size, 1);
    assert_eq!(stats.misses, 1);

    // Second resolution should hit cache
    let _ = fetcher.resolve_dns("localhost").await;
    let stats = fetcher.dns_cache_stats().unwrap();
    assert_eq!(stats.hits, 1);
}

/// Test DNS resolution for URL
#[tokio::test]
async fn test_fetcher_resolve_url_dns() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .with_dns_cache()
        .build(robots_cache)
        .unwrap();

    // Pre-resolve DNS for a URL
    let result = fetcher.resolve_url_dns("http://localhost:8080/path").await;
    assert!(result.is_ok());

    // Check cache was populated
    let stats = fetcher.dns_cache_stats().unwrap();
    assert_eq!(stats.size, 1);
}

/// Test DNS cache clearing via fetcher
#[tokio::test]
async fn test_fetcher_clear_dns_cache() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .with_dns_cache()
        .build(robots_cache)
        .unwrap();

    // Populate cache
    let _ = fetcher.resolve_dns("localhost").await;
    assert_eq!(fetcher.dns_cache_stats().unwrap().size, 1);

    // Clear cache
    fetcher.clear_dns_cache();
    let stats = fetcher.dns_cache_stats().unwrap();
    assert_eq!(stats.size, 0);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 0);
}

/// Test DNS cache hit rate calculation
#[tokio::test]
async fn test_dns_cache_hit_rate() {
    let resolver = CachingDnsResolver::with_defaults().unwrap();

    // Initial hit rate should be 0
    assert_eq!(resolver.cache_hit_rate(), 0.0);

    // One miss
    let _ = resolver.resolve("localhost").await;
    assert_eq!(resolver.cache_hit_rate(), 0.0);

    // One hit
    let _ = resolver.resolve("localhost").await;
    assert_eq!(resolver.cache_hit_rate(), 0.5);

    // Another hit
    let _ = resolver.resolve("localhost").await;
    // 2 hits, 1 miss = 2/3 = 0.666...
    assert!(resolver.cache_hit_rate() > 0.6);
}

/// Test multiple domain resolution
#[tokio::test]
async fn test_dns_multiple_domains() {
    let resolver = CachingDnsResolver::with_defaults().unwrap();

    // Resolve multiple domains
    let _ = resolver.resolve("localhost").await;
    let _ = resolver.resolve("127.0.0.1").await; // IP addresses are valid hostnames

    let stats = resolver.cache_stats();
    assert_eq!(stats.size, 2);
    assert_eq!(stats.misses, 2);

    // Hit both caches
    let _ = resolver.resolve("localhost").await;
    let _ = resolver.resolve("127.0.0.1").await;

    let stats = resolver.cache_stats();
    assert_eq!(stats.hits, 2);
}

/// Test builder with fluent DNS configuration
#[tokio::test]
async fn test_builder_fluent_dns_config() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    // Use fluent API to configure DNS
    let fetcher = HttpFetcherBuilder::new()
        .user_agent("TestBot/1.0")
        .dns_cache_ttl(Duration::from_secs(600))
        .dns_max_cache_size(5000)
        .build(robots_cache)
        .unwrap();

    assert!(fetcher.has_dns_cache());
}

/// Test with_defaults_and_dns constructor
#[tokio::test]
async fn test_fetcher_with_defaults_and_dns() {
    let robots_config = RobotsConfig {
        user_agent: "TestBot/1.0".to_string(),
        ..Default::default()
    };
    let robots_cache = Arc::new(RobotsCache::new(robots_config).unwrap());

    let fetcher = scrapix_crawler::HttpFetcher::with_defaults_and_dns(robots_cache).unwrap();

    assert!(fetcher.has_dns_cache());
}
