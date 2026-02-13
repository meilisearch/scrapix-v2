//! Configuration types for Scrapix crawl jobs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

/// Main configuration for a crawl job
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CrawlConfig {
    /// Starting URLs for the crawl
    #[validate(length(min = 1, message = "At least one start URL is required"))]
    pub start_urls: Vec<String>,

    /// Meilisearch index UID
    #[validate(length(min = 1, message = "Index UID is required"))]
    pub index_uid: String,

    /// Crawler type (http or browser)
    #[serde(default)]
    pub crawler_type: CrawlerType,

    /// Maximum crawl depth from start URLs
    #[serde(default)]
    pub max_depth: Option<u32>,

    /// Maximum number of pages to crawl
    #[serde(default)]
    pub max_pages: Option<u64>,

    /// URL pattern configuration
    #[serde(default)]
    pub url_patterns: UrlPatterns,

    /// Allowed domains for crawling (if empty, inferred from start_urls)
    /// When set, only URLs from these exact domains will be crawled.
    /// This prevents domain explosion (e.g., crawling all Wikipedia languages
    /// when you only want en.wikipedia.org)
    #[serde(default)]
    pub allowed_domains: Vec<String>,

    /// Sitemap configuration
    #[serde(default)]
    pub sitemap: SitemapConfig,

    /// Concurrency settings
    #[serde(default)]
    pub concurrency: ConcurrencyConfig,

    /// Rate limiting settings
    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    /// Proxy configuration
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,

    /// Feature extraction settings
    #[serde(default)]
    pub features: FeaturesConfig,

    /// Meilisearch settings
    pub meilisearch: MeilisearchConfig,

    /// Webhook configurations
    #[serde(default)]
    pub webhooks: Vec<WebhookConfig>,

    /// Additional HTTP headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// User agents to rotate
    #[serde(default)]
    pub user_agents: Vec<String>,
}

/// Type of crawler to use
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerType {
    /// Fast HTTP-only crawler using reqwest
    #[default]
    Http,
    /// Browser-based crawler for JavaScript rendering
    Browser,
}

/// URL filtering patterns
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UrlPatterns {
    /// Glob patterns for URLs to include
    #[serde(default)]
    pub include: Vec<String>,

    /// Glob patterns for URLs to exclude
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Only index URLs matching these patterns (but crawl all)
    #[serde(default)]
    pub index_only: Vec<String>,

    /// Allowed domains for crawling (strict whitelist)
    /// When non-empty, only URLs from these exact domains are allowed.
    /// No subdomain inference or parent domain escapes.
    #[serde(default)]
    pub allowed_domains: Vec<String>,
}

/// Sitemap discovery settings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SitemapConfig {
    /// Whether to discover and use sitemaps
    #[serde(default)]
    pub enabled: bool,

    /// Explicit sitemap URLs to use
    #[serde(default)]
    pub urls: Vec<String>,
}

/// Concurrency settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    /// Maximum concurrent HTTP requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: u32,

    /// Browser pool size for JS rendering
    #[serde(default = "default_browser_pool")]
    pub browser_pool_size: u32,

    /// DNS resolver concurrency
    #[serde(default = "default_dns_concurrency")]
    pub dns_concurrency: u32,
}

fn default_max_concurrent() -> u32 {
    50
}
fn default_browser_pool() -> u32 {
    5
}
fn default_dns_concurrency() -> u32 {
    100
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: default_max_concurrent(),
            browser_pool_size: default_browser_pool(),
            dns_concurrency: default_dns_concurrency(),
        }
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per second (global)
    #[serde(default)]
    pub requests_per_second: Option<f64>,

    /// Maximum requests per minute (global)
    #[serde(default)]
    pub requests_per_minute: Option<u32>,

    /// Minimum delay between requests to same domain (ms)
    #[serde(default = "default_domain_delay")]
    pub per_domain_delay_ms: u64,

    /// Whether to respect robots.txt
    #[serde(default = "default_true")]
    pub respect_robots_txt: bool,

    /// Default crawl delay if not specified in robots.txt (ms)
    #[serde(default = "default_crawl_delay")]
    pub default_crawl_delay_ms: u64,
}

fn default_domain_delay() -> u64 {
    100
}
fn default_true() -> bool {
    true
}
fn default_crawl_delay() -> u64 {
    1000
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: None,
            requests_per_minute: None,
            per_domain_delay_ms: default_domain_delay(),
            respect_robots_txt: true,
            default_crawl_delay_ms: default_crawl_delay(),
        }
    }
}

/// Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// List of proxy URLs
    pub urls: Vec<String>,

    /// Proxy rotation strategy
    #[serde(default)]
    pub rotation: ProxyRotation,

    /// Tiered proxy configuration (fallback tiers)
    #[serde(default)]
    pub tiered: Option<Vec<Vec<String>>>,
}

/// Proxy rotation strategy
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRotation {
    /// Round-robin through proxies
    #[default]
    RoundRobin,
    /// Random proxy selection
    Random,
    /// Use least recently used proxy
    LeastUsed,
}

/// Feature extraction configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeaturesConfig {
    /// Extract meta tags
    #[serde(default)]
    pub metadata: Option<FeatureToggle>,

    /// Convert to Markdown
    #[serde(default)]
    pub markdown: Option<FeatureToggle>,

    /// Split into blocks by headings
    #[serde(default)]
    pub block_split: Option<FeatureToggle>,

    /// Extract schema.org/JSON-LD
    #[serde(default)]
    pub schema: Option<SchemaFeatureConfig>,

    /// Custom CSS selectors
    #[serde(default)]
    pub custom_selectors: Option<CustomSelectorsConfig>,

    /// AI-powered extraction
    #[serde(default)]
    pub ai_extraction: Option<AiExtractionConfig>,

    /// AI-powered summarization
    #[serde(default)]
    pub ai_summary: Option<FeatureToggle>,

    /// Generate embeddings
    #[serde(default)]
    pub embeddings: Option<EmbeddingsConfig>,
}

/// Simple feature toggle with page filters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureToggle {
    /// Whether the feature is enabled
    pub enabled: bool,

    /// Only apply to pages matching these patterns
    #[serde(default)]
    pub include_pages: Vec<String>,

    /// Exclude pages matching these patterns
    #[serde(default)]
    pub exclude_pages: Vec<String>,
}

/// Schema.org extraction settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaFeatureConfig {
    pub enabled: bool,

    /// Only extract specific schema types
    #[serde(default)]
    pub only_types: Vec<String>,

    /// Convert ISO dates to timestamps
    #[serde(default)]
    pub convert_dates: bool,

    #[serde(default)]
    pub include_pages: Vec<String>,

    #[serde(default)]
    pub exclude_pages: Vec<String>,
}

/// Custom CSS selector extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSelectorsConfig {
    pub enabled: bool,

    /// Map of field name to CSS selector(s)
    pub selectors: HashMap<String, SelectorDef>,

    #[serde(default)]
    pub include_pages: Vec<String>,

    #[serde(default)]
    pub exclude_pages: Vec<String>,
}

/// CSS selector definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectorDef {
    /// Single selector
    Single(String),
    /// Multiple selectors (results combined)
    Multiple(Vec<String>),
}

/// AI extraction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiExtractionConfig {
    pub enabled: bool,

    /// Extraction prompt
    pub prompt: String,

    /// Model to use (gpt-4, claude-3, etc.)
    #[serde(default = "default_model")]
    pub model: String,

    /// Maximum tokens in response
    #[serde(default)]
    pub max_tokens: Option<u32>,

    #[serde(default)]
    pub include_pages: Vec<String>,

    #[serde(default)]
    pub exclude_pages: Vec<String>,
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

/// Embeddings generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    pub enabled: bool,

    /// Embedding model to use
    #[serde(default = "default_embedding_model")]
    pub model: String,

    /// Vector dimensions (if model supports it)
    #[serde(default)]
    pub dimensions: Option<u32>,

    #[serde(default)]
    pub include_pages: Vec<String>,

    #[serde(default)]
    pub exclude_pages: Vec<String>,
}

fn default_embedding_model() -> String {
    "text-embedding-3-small".to_string()
}

/// Meilisearch configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct MeilisearchConfig {
    /// Meilisearch URL
    #[validate(url)]
    pub url: String,

    /// API key
    pub api_key: String,

    /// Primary key field name
    #[serde(default)]
    pub primary_key: Option<String>,

    /// Index settings
    #[serde(default)]
    pub settings: Option<MeilisearchSettings>,

    /// Batch size for document indexing
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,

    /// Keep existing settings on re-index
    #[serde(default)]
    pub keep_settings: bool,
}

fn default_batch_size() -> u32 {
    1000
}

/// Meilisearch index settings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeilisearchSettings {
    #[serde(default)]
    pub searchable_attributes: Option<Vec<String>>,

    #[serde(default)]
    pub filterable_attributes: Option<Vec<String>>,

    #[serde(default)]
    pub sortable_attributes: Option<Vec<String>>,

    #[serde(default)]
    pub ranking_rules: Option<Vec<String>>,

    #[serde(default)]
    pub stop_words: Option<Vec<String>>,

    #[serde(default)]
    pub synonyms: Option<HashMap<String, Vec<String>>>,

    #[serde(default)]
    pub distinct_attribute: Option<String>,
}

/// Webhook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Webhook URL
    pub url: String,

    /// Events to send to this webhook
    pub events: Vec<WebhookEvent>,

    /// Authentication configuration
    #[serde(default)]
    pub auth: Option<WebhookAuth>,

    /// Whether webhook is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Request timeout in milliseconds
    #[serde(default = "default_webhook_timeout")]
    pub timeout_ms: u64,

    /// Webhook name for identification
    #[serde(default)]
    pub name: Option<String>,
}

fn default_webhook_timeout() -> u64 {
    30000
}

/// Webhook events
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEvent {
    CrawlStarted,
    CrawlCompleted,
    CrawlFailed,
    ProgressUpdate,
    PageCrawled,
    PageIndexed,
    PageError,
    BatchSent,
}

/// Webhook authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookAuth {
    /// Bearer token authentication
    Bearer { token: String },

    /// HMAC signature authentication
    Hmac {
        secret: String,
        #[serde(default = "default_hmac_algorithm")]
        algorithm: String,
        #[serde(default = "default_hmac_header")]
        header: String,
    },

    /// Custom headers
    Headers { headers: HashMap<String, String> },
}

fn default_hmac_algorithm() -> String {
    "sha256".to_string()
}
fn default_hmac_header() -> String {
    "X-Scrapix-Signature".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CrawlConfig {
            start_urls: vec!["https://example.com".to_string()],
            index_uid: "test".to_string(),
            crawler_type: CrawlerType::default(),
            max_depth: None,
            max_pages: None,
            url_patterns: UrlPatterns::default(),
            allowed_domains: vec![],
            sitemap: SitemapConfig::default(),
            concurrency: ConcurrencyConfig::default(),
            rate_limit: RateLimitConfig::default(),
            proxy: None,
            features: FeaturesConfig::default(),
            meilisearch: MeilisearchConfig {
                url: "http://localhost:7700".to_string(),
                api_key: "masterKey".to_string(),
                primary_key: None,
                settings: None,
                batch_size: default_batch_size(),
                keep_settings: false,
            },
            webhooks: vec![],
            headers: HashMap::new(),
            user_agents: vec![],
        };

        assert_eq!(config.crawler_type, CrawlerType::Http);
        assert_eq!(config.concurrency.max_concurrent_requests, 50);
        assert!(config.rate_limit.respect_robots_txt);
    }

    #[test]
    fn test_deserialize_config() {
        let json = r#"{
            "start_urls": ["https://example.com"],
            "index_uid": "test",
            "crawler_type": "browser",
            "meilisearch": {
                "url": "http://localhost:7700",
                "api_key": "masterKey"
            }
        }"#;

        let config: CrawlConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.crawler_type, CrawlerType::Browser);
        assert_eq!(config.start_urls[0], "https://example.com");
    }
}
