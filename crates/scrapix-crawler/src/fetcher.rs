//! HTTP page fetcher with connection pooling, compression, and retry logic

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE,
        IF_MODIFIED_SINCE, IF_NONE_MATCH,
    },
    redirect::Policy,
    Client, Response, StatusCode,
};
use tracing::{debug, instrument};
use url::Url;

use scrapix_core::{CrawlUrl, RawPage, Result, ScrapixError};

use crate::dns::{CachingDnsResolver, DnsCacheStats, DnsConfig};
use crate::robots::RobotsCache;

/// Conditional request headers for incremental crawling
#[derive(Debug, Clone, Default)]
pub struct ConditionalRequestHeaders {
    /// ETag from previous response (for If-None-Match header)
    pub etag: Option<String>,
    /// Last-Modified from previous response (for If-Modified-Since header)
    pub last_modified: Option<String>,
}

impl ConditionalRequestHeaders {
    /// Create new conditional headers
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the ETag
    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.etag = Some(etag.into());
        self
    }

    /// Set the Last-Modified header
    pub fn with_last_modified(mut self, last_modified: impl Into<String>) -> Self {
        self.last_modified = Some(last_modified.into());
        self
    }

    /// Check if any conditional headers are set
    pub fn has_headers(&self) -> bool {
        self.etag.is_some() || self.last_modified.is_some()
    }
}

/// Result of a conditional fetch operation
#[derive(Debug)]
pub enum FetchResult {
    /// Content was fetched (status 200 or similar)
    Fetched(RawPage),
    /// Content not modified (status 304)
    NotModified {
        /// The URL that was checked
        url: String,
        /// Duration of the request
        fetch_duration_ms: u64,
    },
}

/// Configuration for the HTTP fetcher
#[derive(Debug, Clone)]
pub struct FetcherConfig {
    /// User agent string
    pub user_agent: String,
    /// Request timeout
    pub timeout: Duration,
    /// Connect timeout
    pub connect_timeout: Duration,
    /// Maximum redirects to follow
    pub max_redirects: usize,
    /// Whether to accept invalid SSL certificates
    pub accept_invalid_certs: bool,
    /// Maximum response body size in bytes
    pub max_body_size: usize,
    /// Whether to follow redirects
    pub follow_redirects: bool,
    /// Custom headers to include in requests
    pub custom_headers: HashMap<String, String>,
    /// Retry configuration
    pub retry_config: RetryConfig,
}

impl Default for FetcherConfig {
    fn default() -> Self {
        Self {
            user_agent: "Scrapix/1.0 (compatible; +https://github.com/quentindequelen/scrapix)"
                .to_string(),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            max_redirects: 10,
            accept_invalid_certs: false,
            max_body_size: 10 * 1024 * 1024, // 10MB
            follow_redirects: true,
            custom_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
        }
    }
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier
    pub backoff_multiplier: f64,
    /// Status codes that should trigger a retry
    pub retryable_status_codes: Vec<u16>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            retryable_status_codes: vec![429, 500, 502, 503, 504],
        }
    }
}

/// HTTP fetcher implementation
pub struct HttpFetcher {
    client: Client,
    config: FetcherConfig,
    robots_cache: Arc<RobotsCache>,
    dns_resolver: Option<Arc<CachingDnsResolver>>,
}

impl HttpFetcher {
    /// Create a new HTTP fetcher with the given configuration
    pub fn new(config: FetcherConfig, robots_cache: Arc<RobotsCache>) -> Result<Self> {
        Self::new_with_dns(config, robots_cache, None)
    }

    /// Create a new HTTP fetcher with DNS caching enabled
    pub fn new_with_dns(
        config: FetcherConfig,
        robots_cache: Arc<RobotsCache>,
        dns_resolver: Option<Arc<CachingDnsResolver>>,
    ) -> Result<Self> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/markdown,text/html;q=0.9,application/xhtml+xml;q=0.9,application/xml;q=0.8,*/*;q=0.1",
            ),
        );
        default_headers.insert(
            ACCEPT_ENCODING,
            HeaderValue::from_static("gzip, deflate, br"),
        );
        default_headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

        // Add custom headers
        for (name, value) in &config.custom_headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::try_from(name.as_str()),
                HeaderValue::from_str(value),
            ) {
                default_headers.insert(name, value);
            }
        }

        let redirect_policy = if config.follow_redirects {
            Policy::limited(config.max_redirects)
        } else {
            Policy::none()
        };

        let client = Client::builder()
            .user_agent(&config.user_agent)
            .timeout(config.timeout)
            .connect_timeout(config.connect_timeout)
            .redirect(redirect_policy)
            .danger_accept_invalid_certs(config.accept_invalid_certs)
            .default_headers(default_headers)
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .pool_max_idle_per_host(200)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .tcp_nodelay(true)
            .build()
            .map_err(|e| ScrapixError::Crawl(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client,
            config,
            robots_cache,
            dns_resolver,
        })
    }

    /// Create a new HTTP fetcher with default configuration
    pub fn with_defaults(robots_cache: Arc<RobotsCache>) -> Result<Self> {
        Self::new(FetcherConfig::default(), robots_cache)
    }

    /// Create a new HTTP fetcher with default configuration and DNS caching
    pub fn with_defaults_and_dns(robots_cache: Arc<RobotsCache>) -> Result<Self> {
        let dns_resolver = Arc::new(CachingDnsResolver::with_defaults()?);
        Self::new_with_dns(FetcherConfig::default(), robots_cache, Some(dns_resolver))
    }

    /// Fetch a URL with retry logic
    #[instrument(skip(self), fields(url = %url.url))]
    pub async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage> {
        let parsed_url = Url::parse(&url.url)?;

        // Check robots.txt
        if !self.robots_cache.is_allowed(&url.url).await? {
            return Err(ScrapixError::RobotsDisallowed {
                url: url.url.clone(),
            });
        }

        let mut last_error = None;
        let mut backoff = self.config.retry_config.initial_backoff;

        for attempt in 0..=self.config.retry_config.max_retries {
            if attempt > 0 {
                debug!(attempt, "Retrying request after {:?}", backoff);
                tokio::time::sleep(backoff).await;
                backoff = Duration::from_secs_f64(
                    (backoff.as_secs_f64() * self.config.retry_config.backoff_multiplier)
                        .min(self.config.retry_config.max_backoff.as_secs_f64()),
                );
            }

            let start = Instant::now();

            match self.fetch_once(&parsed_url).await {
                Ok((response, final_url)) => {
                    let fetch_duration = start.elapsed();
                    return self
                        .process_response(url, response, final_url, fetch_duration)
                        .await;
                }
                Err(e) => {
                    // Check if this is a non-retryable HTTP error
                    if let ScrapixError::Http { status, .. } = e {
                        if !self
                            .config
                            .retry_config
                            .retryable_status_codes
                            .contains(&status)
                        {
                            return Err(ScrapixError::Http {
                                status,
                                url: url.url.clone(),
                            });
                        }
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ScrapixError::Crawl(format!("Failed to fetch {} after retries", url.url))
        }))
    }

    /// Fetch a URL with conditional headers for incremental crawling
    ///
    /// This method sends If-None-Match and If-Modified-Since headers if provided,
    /// allowing the server to return 304 Not Modified if content hasn't changed.
    #[instrument(skip(self), fields(url = %url.url))]
    pub async fn fetch_conditional(
        &self,
        url: &CrawlUrl,
        conditional_headers: &ConditionalRequestHeaders,
    ) -> Result<FetchResult> {
        let parsed_url = Url::parse(&url.url)?;

        // Check robots.txt
        if !self.robots_cache.is_allowed(&url.url).await? {
            return Err(ScrapixError::RobotsDisallowed {
                url: url.url.clone(),
            });
        }

        let mut last_error = None;
        let mut backoff = self.config.retry_config.initial_backoff;

        for attempt in 0..=self.config.retry_config.max_retries {
            if attempt > 0 {
                debug!(attempt, "Retrying request after {:?}", backoff);
                tokio::time::sleep(backoff).await;
                backoff = Duration::from_secs_f64(
                    (backoff.as_secs_f64() * self.config.retry_config.backoff_multiplier)
                        .min(self.config.retry_config.max_backoff.as_secs_f64()),
                );
            }

            let start = Instant::now();

            match self
                .fetch_once_conditional(&parsed_url, conditional_headers)
                .await
            {
                Ok((response, final_url)) => {
                    let fetch_duration = start.elapsed();

                    // Check for 304 Not Modified
                    if response.status() == StatusCode::NOT_MODIFIED {
                        debug!(url = %url.url, "Content not modified (304)");
                        return Ok(FetchResult::NotModified {
                            url: url.url.clone(),
                            fetch_duration_ms: fetch_duration.as_millis() as u64,
                        });
                    }

                    let page = self
                        .process_response(url, response, final_url, fetch_duration)
                        .await?;
                    return Ok(FetchResult::Fetched(page));
                }
                Err(e) => {
                    // Check if this is a non-retryable HTTP error
                    if let ScrapixError::Http { status, .. } = e {
                        if !self
                            .config
                            .retry_config
                            .retryable_status_codes
                            .contains(&status)
                        {
                            return Err(ScrapixError::Http {
                                status,
                                url: url.url.clone(),
                            });
                        }
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ScrapixError::Crawl(format!("Failed to fetch {} after retries", url.url))
        }))
    }

    /// Perform a single fetch attempt
    async fn fetch_once(&self, url: &Url) -> Result<(Response, String)> {
        // Pre-resolve DNS if resolver is configured (warms cache for future requests)
        if let Some(ref resolver) = self.dns_resolver {
            if let Some(host) = url.host_str() {
                // Pre-resolve to warm the cache; we don't fail on DNS errors here
                // since reqwest will still try to resolve
                if let Err(e) = resolver.resolve(host).await {
                    debug!(host, error = %e, "DNS pre-resolution failed, reqwest will resolve");
                }
            }
        }

        let response = self.client.get(url.as_str()).send().await.map_err(|e| {
            if e.is_timeout() {
                ScrapixError::Timeout(format!("Request timed out: {}", url))
            } else if e.is_connect() {
                ScrapixError::Connection(format!("Connection failed: {}", e))
            } else if let Some(status) = e.status() {
                ScrapixError::Http {
                    status: status.as_u16(),
                    url: url.to_string(),
                }
            } else {
                ScrapixError::Network(e.to_string())
            }
        })?;
        let final_url = response.url().to_string();
        Ok((response, final_url))
    }

    /// Perform a single fetch attempt with conditional headers
    async fn fetch_once_conditional(
        &self,
        url: &Url,
        conditional_headers: &ConditionalRequestHeaders,
    ) -> Result<(Response, String)> {
        // Pre-resolve DNS if resolver is configured (warms cache for future requests)
        if let Some(ref resolver) = self.dns_resolver {
            if let Some(host) = url.host_str() {
                if let Err(e) = resolver.resolve(host).await {
                    debug!(host, error = %e, "DNS pre-resolution failed, reqwest will resolve");
                }
            }
        }

        let mut request = self.client.get(url.as_str());

        // Add conditional headers if present
        if let Some(ref etag) = conditional_headers.etag {
            if let Ok(value) = HeaderValue::from_str(etag) {
                request = request.header(IF_NONE_MATCH, value);
            }
        }
        if let Some(ref last_modified) = conditional_headers.last_modified {
            if let Ok(value) = HeaderValue::from_str(last_modified) {
                request = request.header(IF_MODIFIED_SINCE, value);
            }
        }

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ScrapixError::Timeout(format!("Request timed out: {}", url))
            } else if e.is_connect() {
                ScrapixError::Connection(format!("Connection failed: {}", e))
            } else if let Some(status) = e.status() {
                ScrapixError::Http {
                    status: status.as_u16(),
                    url: url.to_string(),
                }
            } else {
                ScrapixError::Network(e.to_string())
            }
        })?;
        let final_url = response.url().to_string();
        Ok((response, final_url))
    }

    /// Process the response into a RawPage
    async fn process_response(
        &self,
        crawl_url: &CrawlUrl,
        response: Response,
        final_url: String,
        fetch_duration: Duration,
    ) -> Result<RawPage> {
        let status = response.status().as_u16();

        // Convert headers
        let mut headers = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                headers.insert(name.to_string(), v.to_string());
            }
        }

        let content_type = headers.get("content-type").cloned();

        // Check content type - we accept HTML and markdown
        if let Some(ref ct) = content_type {
            if !ct.contains("text/html")
                && !ct.contains("application/xhtml")
                && !ct.contains("text/markdown")
            {
                return Err(ScrapixError::Crawl(format!(
                    "Unsupported content type: {}",
                    ct
                )));
            }
        }

        // Read body with size limit
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ScrapixError::Network(format!("Failed to read response body: {}", e)))?;

        if bytes.len() > self.config.max_body_size {
            return Err(ScrapixError::Crawl(format!(
                "Response body too large: {} bytes (max: {})",
                bytes.len(),
                self.config.max_body_size
            )));
        }

        // Decode as UTF-8 (with fallback)
        let html = String::from_utf8_lossy(&bytes).into_owned();

        Ok(RawPage {
            url: crawl_url.url.clone(),
            final_url,
            status,
            headers,
            html,
            content_type,
            js_rendered: false,
            fetched_at: Utc::now(),
            fetch_duration_ms: fetch_duration.as_millis() as u64,
        })
    }

    /// Check if a URL is allowed by robots.txt
    pub async fn is_allowed(&self, url: &str) -> Result<bool> {
        self.robots_cache.is_allowed(url).await
    }

    /// Get crawl delay for a domain
    pub async fn get_crawl_delay(&self, domain: &str) -> Result<Option<u64>> {
        self.robots_cache.get_crawl_delay(domain).await
    }

    /// Pre-resolve DNS for a hostname (warms the cache)
    ///
    /// This can be called before fetching to ensure DNS is cached.
    /// Returns the resolved IP addresses.
    pub async fn resolve_dns(&self, hostname: &str) -> Result<Vec<std::net::IpAddr>> {
        if let Some(ref resolver) = self.dns_resolver {
            resolver.resolve(hostname).await
        } else {
            Err(ScrapixError::Crawl(
                "DNS resolver not configured".to_string(),
            ))
        }
    }

    /// Pre-resolve DNS for a URL (warms the cache)
    pub async fn resolve_url_dns(&self, url: &str) -> Result<Vec<std::net::IpAddr>> {
        let parsed = Url::parse(url)?;
        if let Some(host) = parsed.host_str() {
            self.resolve_dns(host).await
        } else {
            Err(ScrapixError::Crawl(format!("No host in URL: {}", url)))
        }
    }

    /// Get DNS cache statistics
    pub fn dns_cache_stats(&self) -> Option<DnsCacheStats> {
        self.dns_resolver.as_ref().map(|r| r.cache_stats())
    }

    /// Clear the DNS cache
    pub fn clear_dns_cache(&self) {
        if let Some(ref resolver) = self.dns_resolver {
            resolver.clear_cache();
        }
    }

    /// Check if DNS caching is enabled
    pub fn has_dns_cache(&self) -> bool {
        self.dns_resolver.is_some()
    }
}

/// Trait implementation for the core Fetcher trait
#[async_trait]
impl scrapix_core::traits::Fetcher for HttpFetcher {
    async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage> {
        HttpFetcher::fetch(self, url).await
    }

    async fn is_allowed(&self, url: &str) -> Result<bool> {
        self.is_allowed(url).await
    }

    async fn get_crawl_delay(&self, domain: &str) -> Result<Option<u64>> {
        self.get_crawl_delay(domain).await
    }
}

/// Builder for HttpFetcher
pub struct HttpFetcherBuilder {
    config: FetcherConfig,
    dns_config: Option<DnsConfig>,
}

impl HttpFetcherBuilder {
    pub fn new() -> Self {
        Self {
            config: FetcherConfig::default(),
            dns_config: None,
        }
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = user_agent.into();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.config.connect_timeout = timeout;
        self
    }

    pub fn max_redirects(mut self, max: usize) -> Self {
        self.config.max_redirects = max;
        self
    }

    pub fn accept_invalid_certs(mut self, accept: bool) -> Self {
        self.config.accept_invalid_certs = accept;
        self
    }

    pub fn max_body_size(mut self, size: usize) -> Self {
        self.config.max_body_size = size;
        self
    }

    pub fn follow_redirects(mut self, follow: bool) -> Self {
        self.config.follow_redirects = follow;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.custom_headers.insert(name.into(), value.into());
        self
    }

    pub fn max_retries(mut self, max: u32) -> Self {
        self.config.retry_config.max_retries = max;
        self
    }

    /// Enable DNS caching with default configuration
    pub fn with_dns_cache(mut self) -> Self {
        self.dns_config = Some(DnsConfig::default());
        self
    }

    /// Enable DNS caching with custom configuration
    pub fn with_dns_config(mut self, config: DnsConfig) -> Self {
        self.dns_config = Some(config);
        self
    }

    /// Set DNS cache TTL
    pub fn dns_cache_ttl(mut self, ttl: Duration) -> Self {
        let config = self.dns_config.get_or_insert_with(DnsConfig::default);
        config.cache_ttl = ttl;
        self
    }

    /// Set maximum DNS cache size
    pub fn dns_max_cache_size(mut self, size: usize) -> Self {
        let config = self.dns_config.get_or_insert_with(DnsConfig::default);
        config.max_cache_size = size;
        self
    }

    pub fn build(self, robots_cache: Arc<RobotsCache>) -> Result<HttpFetcher> {
        let dns_resolver = if let Some(dns_config) = self.dns_config {
            Some(Arc::new(CachingDnsResolver::new(dns_config)?))
        } else {
            None
        };
        HttpFetcher::new_with_dns(self.config, robots_cache, dns_resolver)
    }
}

impl Default for HttpFetcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetcher_config_default() {
        let config = FetcherConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_redirects, 10);
        assert!(!config.accept_invalid_certs);
    }

    #[test]
    fn test_builder() {
        let builder = HttpFetcherBuilder::new()
            .user_agent("TestBot/1.0")
            .timeout(Duration::from_secs(60))
            .max_redirects(5)
            .header("X-Custom", "value");

        assert_eq!(builder.config.user_agent, "TestBot/1.0");
        assert_eq!(builder.config.timeout, Duration::from_secs(60));
        assert_eq!(builder.config.max_redirects, 5);
        assert_eq!(
            builder.config.custom_headers.get("X-Custom"),
            Some(&"value".to_string())
        );
    }
}
