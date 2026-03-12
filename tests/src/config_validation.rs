//! Config Validation Edge Cases (P0/P1)
//!
//! Tests that configuration parsing, defaults, and backward compatibility
//! work correctly. Bad config → domain explosion, silent feature loss, or crashes.

use scrapix_core::{
    AiExtractionConfig, ConcurrencyConfig, CrawlConfig, CrawlerType, CustomSelectorsConfig,
    FeatureToggle, FeaturesConfig, IndexStrategy, MeilisearchConfig, RateLimitConfig,
    SchemaFeatureConfig, SelectorDef, UrlPatterns,
};
use std::collections::HashMap;
use validator::Validate;

// ============================================================================
// Default sanity tests
// ============================================================================

fn minimal_config() -> CrawlConfig {
    CrawlConfig {
        start_urls: vec!["https://example.com".to_string()],
        index_uid: "test".to_string(),
        source: None,
        crawler_type: CrawlerType::default(),
        max_depth: None,
        max_pages: None,
        url_patterns: UrlPatterns::default(),
        allowed_domains: vec![],
        sitemap: Default::default(),
        concurrency: ConcurrencyConfig::default(),
        rate_limit: RateLimitConfig::default(),
        proxy: None,
        features: FeaturesConfig::default(),
        meilisearch: MeilisearchConfig {
            url: "http://localhost:7700".to_string(),
            api_key: "masterKey".to_string(),
            primary_key: None,
            settings: None,
            batch_size: 1000,
            keep_settings: false,
        },
        webhooks: vec![],
        headers: HashMap::new(),
        user_agents: vec![],
        index_strategy: IndexStrategy::default(),
    }
}

#[test]
fn test_defaults_are_polite() {
    let config = minimal_config();
    // Rate limiting should be on by default
    assert!(config.rate_limit.respect_robots_txt);
    assert!(config.rate_limit.per_domain_delay_ms >= 100);
    assert_eq!(config.rate_limit.default_crawl_delay_ms, 1000);
}

#[test]
fn test_defaults_concurrency_reasonable() {
    let config = ConcurrencyConfig::default();
    assert_eq!(config.max_concurrent_requests, 50);
    assert_eq!(config.browser_pool_size, 5);
}

#[test]
fn test_default_index_strategy_is_update() {
    assert_eq!(IndexStrategy::default(), IndexStrategy::Update);
    assert!(!IndexStrategy::Update.is_replace());
    assert!(IndexStrategy::Replace.is_replace());
}

// ============================================================================
// Deserialization tests
// ============================================================================

#[test]
fn test_config_deserialize_minimal_json() {
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "meilisearch": {
            "url": "http://localhost:7700",
            "api_key": "masterKey"
        }
    }"#;

    let config: CrawlConfig = serde_json::from_str(json).expect("minimal config should parse");
    assert_eq!(config.start_urls.len(), 1);
    assert_eq!(config.crawler_type, CrawlerType::Http); // default
    assert!(config.max_depth.is_none());
    assert!(config.max_pages.is_none());
    assert_eq!(config.index_strategy, IndexStrategy::Update); // default
}

#[test]
fn test_config_deserialize_all_features() {
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "crawler_type": "browser",
        "max_depth": 5,
        "max_pages": 1000,
        "features": {
            "metadata": {"enabled": true},
            "markdown": {"enabled": true},
            "block_split": {"enabled": true},
            "schema": {"enabled": true, "only_types": ["Article"]},
            "ai_summary": {"enabled": true},
            "ai_extraction": {"enabled": true, "prompt": "Extract product info"}
        },
        "meilisearch": {
            "url": "http://localhost:7700",
            "api_key": "masterKey"
        }
    }"#;

    let config: CrawlConfig = serde_json::from_str(json).expect("full config should parse");
    assert_eq!(config.crawler_type, CrawlerType::Browser);
    assert_eq!(config.max_depth, Some(5));
    assert_eq!(config.max_pages, Some(1000));
    assert!(config.features.metadata_enabled());
    assert!(config.features.markdown_enabled());
    assert!(config.features.block_split_enabled());
    assert!(config.features.schema_enabled());
    assert!(config.features.ai_summary_enabled());
    assert!(config.features.ai_extraction_enabled());
}

// ============================================================================
// Index strategy backward compatibility
// ============================================================================

#[test]
fn test_index_strategy_deserialize_string_update() {
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "index_strategy": "update",
        "meilisearch": {"url": "http://localhost:7700", "api_key": "key"}
    }"#;

    let config: CrawlConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.index_strategy, IndexStrategy::Update);
}

#[test]
fn test_index_strategy_deserialize_string_replace() {
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "index_strategy": "replace",
        "meilisearch": {"url": "http://localhost:7700", "api_key": "key"}
    }"#;

    let config: CrawlConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.index_strategy, IndexStrategy::Replace);
}

#[test]
fn test_index_strategy_deserialize_legacy_bool_true() {
    // Legacy format: "replace_index": true
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "replace_index": true,
        "meilisearch": {"url": "http://localhost:7700", "api_key": "key"}
    }"#;

    let config: CrawlConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.index_strategy, IndexStrategy::Replace);
}

#[test]
fn test_index_strategy_deserialize_legacy_bool_false() {
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "replace_index": false,
        "meilisearch": {"url": "http://localhost:7700", "api_key": "key"}
    }"#;

    let config: CrawlConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.index_strategy, IndexStrategy::Update);
}

#[test]
fn test_index_strategy_deserialize_invalid_string_errors() {
    let json = r#"{
        "start_urls": ["https://example.com"],
        "index_uid": "test",
        "index_strategy": "invalid_value",
        "meilisearch": {"url": "http://localhost:7700", "api_key": "key"}
    }"#;

    let result: Result<CrawlConfig, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// ============================================================================
// Validation tests
// ============================================================================

#[test]
fn test_config_validation_rejects_empty_start_urls() {
    let mut config = minimal_config();
    config.start_urls = vec![];

    let result = config.validate();
    assert!(result.is_err());
}

#[test]
fn test_config_validation_rejects_empty_index_uid() {
    let mut config = minimal_config();
    config.index_uid = "".to_string();

    let result = config.validate();
    assert!(result.is_err());
}

// ============================================================================
// Features config tests
// ============================================================================

#[test]
fn test_features_all_disabled_by_default() {
    let features = FeaturesConfig::default();
    assert!(!features.metadata_enabled());
    assert!(!features.markdown_enabled());
    assert!(!features.block_split_enabled());
    assert!(!features.schema_enabled());
    assert!(!features.custom_selectors_enabled());
    assert!(!features.ai_extraction_enabled());
    assert!(!features.ai_summary_enabled());
}

#[test]
fn test_features_enabled_returns_false_when_none() {
    let features = FeaturesConfig {
        metadata: None,
        markdown: None,
        block_split: None,
        schema: None,
        custom_selectors: None,
        ai_extraction: None,
        ai_summary: None,
    };

    assert!(!features.metadata_enabled());
    assert!(!features.markdown_enabled());
}

#[test]
fn test_features_enabled_returns_false_when_disabled() {
    let features = FeaturesConfig {
        metadata: Some(FeatureToggle {
            enabled: false,
            include_pages: vec![],
            exclude_pages: vec![],
        }),
        ..Default::default()
    };

    assert!(!features.metadata_enabled());
}

#[test]
fn test_features_from_cli_args_all_enabled() {
    let features = FeaturesConfig::from_cli_args(
        true,
        true,
        true,
        true,
        true,
        true,
        Some("Extract products".to_string()),
    );

    assert!(features.metadata_enabled());
    assert!(features.markdown_enabled());
    assert!(features.schema_enabled());
    assert!(features.block_split_enabled());
    assert!(features.ai_summary_enabled());
    assert!(features.ai_extraction_enabled());

    // Check the prompt is set
    let ai = features.ai_extraction.unwrap();
    assert_eq!(ai.prompt, "Extract products");
}

#[test]
fn test_features_from_cli_args_all_disabled() {
    let features = FeaturesConfig::from_cli_args(false, false, false, false, false, false, None);

    assert!(!features.metadata_enabled());
    assert!(!features.markdown_enabled());
    assert!(!features.schema_enabled());
    assert!(!features.block_split_enabled());
    assert!(!features.ai_summary_enabled());
    assert!(!features.ai_extraction_enabled());
}

#[test]
fn test_features_config_round_trip() {
    let features = FeaturesConfig {
        metadata: Some(FeatureToggle {
            enabled: true,
            include_pages: vec!["https://example.com/docs/**".to_string()],
            exclude_pages: vec!["**/_draft/**".to_string()],
        }),
        markdown: Some(FeatureToggle {
            enabled: true,
            include_pages: vec![],
            exclude_pages: vec![],
        }),
        block_split: Some(FeatureToggle {
            enabled: true,
            include_pages: vec![],
            exclude_pages: vec![],
        }),
        schema: Some(SchemaFeatureConfig {
            enabled: true,
            only_types: vec!["Article".to_string(), "Product".to_string()],
            convert_dates: true,
            include_pages: vec![],
            exclude_pages: vec![],
        }),
        custom_selectors: Some(CustomSelectorsConfig {
            enabled: true,
            selectors: {
                let mut m = HashMap::new();
                m.insert(
                    "price".to_string(),
                    SelectorDef::Single(".price".to_string()),
                );
                m.insert(
                    "tags".to_string(),
                    SelectorDef::Multiple(vec![".tag".to_string(), ".label".to_string()]),
                );
                m
            },
            include_pages: vec![],
            exclude_pages: vec![],
        }),
        ai_extraction: Some(AiExtractionConfig {
            enabled: true,
            prompt: "Extract structured data".to_string(),
            include_pages: vec![],
            exclude_pages: vec![],
        }),
        ai_summary: Some(FeatureToggle {
            enabled: true,
            include_pages: vec![],
            exclude_pages: vec![],
        }),
    };

    let json = serde_json::to_string(&features).expect("serialize features");
    let d: FeaturesConfig = serde_json::from_str(&json).expect("deserialize features");

    assert!(d.metadata_enabled());
    assert!(d.markdown_enabled());
    assert!(d.block_split_enabled());
    assert!(d.schema_enabled());
    assert!(d.custom_selectors_enabled());
    assert!(d.ai_extraction_enabled());
    assert!(d.ai_summary_enabled());

    // Check nested fields survived
    let schema = d.schema.unwrap();
    assert_eq!(schema.only_types.len(), 2);
    assert!(schema.convert_dates);

    let selectors = d.custom_selectors.unwrap();
    assert_eq!(selectors.selectors.len(), 2);

    let ai = d.ai_extraction.unwrap();
    assert_eq!(ai.prompt, "Extract structured data");
}

// ============================================================================
// URL patterns tests
// ============================================================================

#[test]
fn test_url_patterns_default_is_empty() {
    let patterns = UrlPatterns::default();
    assert!(patterns.include.is_empty());
    assert!(patterns.exclude.is_empty());
    assert!(patterns.index_only.is_empty());
    assert!(patterns.allowed_domains.is_empty());
}

#[test]
fn test_url_patterns_round_trip() {
    let patterns = UrlPatterns {
        include: vec!["https://example.com/docs/**".to_string()],
        exclude: vec!["**/_private/**".to_string()],
        index_only: vec!["https://example.com/docs/public/**".to_string()],
        allowed_domains: vec!["example.com".to_string()],
    };

    let json = serde_json::to_string(&patterns).expect("serialize");
    let d: UrlPatterns = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(d.include.len(), 1);
    assert_eq!(d.exclude.len(), 1);
    assert_eq!(d.index_only.len(), 1);
    assert_eq!(d.allowed_domains.len(), 1);
}

// ============================================================================
// CrawlerType tests
// ============================================================================

#[test]
fn test_crawler_type_default_is_http() {
    assert_eq!(CrawlerType::default(), CrawlerType::Http);
}

#[test]
fn test_crawler_type_deserialize() {
    let http: CrawlerType = serde_json::from_str(r#""http""#).unwrap();
    let browser: CrawlerType = serde_json::from_str(r#""browser""#).unwrap();

    assert_eq!(http, CrawlerType::Http);
    assert_eq!(browser, CrawlerType::Browser);
}
