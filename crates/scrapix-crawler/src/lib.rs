//! # Scrapix Crawler
//!
//! HTTP and browser-based web crawler.
//!
//! ## Features
//!
//! - High-performance HTTP fetching with reqwest
//! - Connection pooling
//! - Proxy rotation
//! - DNS caching
//! - Robots.txt compliance
//! - URL extraction with pattern filtering
//!
//! ## Browser Rendering (Optional)
//!
//! Enable browser rendering with feature flags:
//!
//! - `browser-cdp`: Chrome/Chromium via CDP (chromiumoxide)
//! - `browser-webdriver`: WebDriver protocol (fantoccini)
//! - `browser-full`: All browser options
//!
//! ### Rendering Strategy
//!
//! 1. **HTTP First** (default): Fast reqwest + scraper for 80-90% of pages
//! 2. **Lightpanda** (future): 10x faster than Chrome when stable
//! 3. **Chrome/Chromium**: Fallback for complex JS sites
//! 4. **Browserbase** (optional): Hosted infrastructure for scale
//!
//! ## Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use scrapix_crawler::{
//!     fetcher::{HttpFetcher, HttpFetcherBuilder},
//!     robots::RobotsCache,
//!     extractor::UrlExtractor,
//! };
//! use scrapix_core::CrawlUrl;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create robots.txt cache
//!     let robots_cache = Arc::new(RobotsCache::with_defaults()?);
//!
//!     // Create HTTP fetcher
//!     let fetcher = HttpFetcherBuilder::new()
//!         .user_agent("MyBot/1.0")
//!         .build(robots_cache)?;
//!
//!     // Fetch a page
//!     let url = CrawlUrl::seed("https://example.com");
//!     let page = fetcher.fetch(&url).await?;
//!
//!     // Extract links
//!     let extractor = UrlExtractor::with_defaults();
//!     let links = extractor.extract(&page, 0);
//!
//!     println!("Found {} links", links.len());
//!     Ok(())
//! }
//! ```

pub mod dns;
pub mod extractor;
pub mod fetcher;
pub mod proxy;
pub mod robots;
pub mod sitemap;

#[cfg(feature = "browser-cdp")]
pub mod renderer_cdp;

#[cfg(feature = "browser-webdriver")]
pub mod renderer_webdriver;

// Re-exports for convenience
pub use dns::{CachingDnsResolver, DnsCacheStats, DnsConfig};
pub use extractor::{ExtractorConfig, UrlExtractor, UrlExtractorBuilder};
pub use fetcher::{
    ConditionalRequestHeaders, FetchResult, FetcherConfig, HttpFetcher, HttpFetcherBuilder,
    RetryConfig,
};
pub use proxy::{ProxyConfig, ProxyPool, RotationStrategy};
pub use robots::{
    PersistentRobotsCache, PersistentRobotsEntry, RobotsCache, RobotsCacheStats, RobotsConfig,
    RobotsPersistence, RocksDbOps, RocksRobotsPersistence,
};
pub use sitemap::{
    ChangeFrequency, SitemapConfig, SitemapContent, SitemapEntry, SitemapParser, SitemapUrl,
};

// CDP renderer re-exports
#[cfg(feature = "browser-cdp")]
pub use renderer_cdp::{
    CdpConfig, CdpError, CdpRenderer, CdpRendererBuilder, RenderResult, WaitUntil,
};

// WebDriver renderer re-exports
#[cfg(feature = "browser-webdriver")]
pub use renderer_webdriver::{
    BrowserType, PageLoadStrategy, WebDriverConfig, WebDriverCookie, WebDriverError,
    WebDriverRenderResult, WebDriverRenderer, WebDriverRendererBuilder,
};
