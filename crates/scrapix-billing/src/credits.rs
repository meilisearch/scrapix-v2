//! Credit calculation for all Scrapix operations.
//!
//! Pure functions with no side effects — safe to call from anywhere.

use scrapix_core::{CrawlerType, FeaturesConfig};

/// Compute credits for a /scrape request.
///
/// - `feature_format_count`: number of feature formats requested
///   (Markdown, Links, Metadata, Screenshot, Schema, Blocks).
///   Base formats (Html, RawHtml, Content) are free and should NOT be counted.
/// - AI summary: +5
/// - AI extraction: +5
/// - Minimum: 1 credit (even if no features)
pub fn scrape_credits(
    feature_format_count: i64,
    has_ai_summary: bool,
    has_ai_extraction: bool,
) -> i64 {
    let ai_cost = if has_ai_summary { 5 } else { 0 } + if has_ai_extraction { 5 } else { 0 };

    // At least 1 credit per scrape
    (feature_format_count + ai_cost).max(1)
}

/// Compute per-page credits for a /crawl job.
///
/// - HTTP mode: 1 base per page
/// - Browser (JS) mode: 2 base per page
/// - +1 per enabled feature (metadata, markdown, block_split, schema, custom_selectors)
/// - +5 for AI extraction
/// - +5 for AI summary
pub fn crawl_credits_per_page(crawler_type: &CrawlerType, features: &FeaturesConfig) -> i64 {
    let base = match crawler_type {
        CrawlerType::Http => 1,
        CrawlerType::Browser => 2,
    };

    let mut feature_count: i64 = 0;
    if features.metadata.as_ref().is_some_and(|f| f.enabled) {
        feature_count += 1;
    }
    if features.markdown.as_ref().is_some_and(|f| f.enabled) {
        feature_count += 1;
    }
    if features.block_split.as_ref().is_some_and(|f| f.enabled) {
        feature_count += 1;
    }
    if features.schema.as_ref().is_some_and(|s| s.enabled) {
        feature_count += 1;
    }
    if features
        .custom_selectors
        .as_ref()
        .is_some_and(|s| s.enabled)
    {
        feature_count += 1;
    }
    if features.ai_extraction.as_ref().is_some_and(|a| a.enabled) {
        feature_count += 5;
    }
    if features.ai_summary.as_ref().is_some_and(|f| f.enabled) {
        feature_count += 5;
    }

    base + feature_count
}

/// Map credits: flat 2 per call.
pub const MAP_CREDITS: i64 = 2;

/// Search credits: flat 2 per call.
pub const SEARCH_CREDITS: i64 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrape_credits_minimum_one() {
        assert_eq!(scrape_credits(0, false, false), 1);
    }

    #[test]
    fn test_scrape_credits_base_formats_free() {
        // Base formats aren't counted, so feature_format_count = 0
        assert_eq!(scrape_credits(0, false, false), 1);
    }

    #[test]
    fn test_scrape_credits_feature_formats() {
        assert_eq!(scrape_credits(1, false, false), 1);
        assert_eq!(scrape_credits(3, false, false), 3);
        assert_eq!(scrape_credits(6, false, false), 6);
    }

    #[test]
    fn test_scrape_credits_ai_summary() {
        assert_eq!(scrape_credits(0, true, false), 5);
    }

    #[test]
    fn test_scrape_credits_ai_extraction() {
        assert_eq!(scrape_credits(0, false, true), 5);
    }

    #[test]
    fn test_scrape_credits_ai_both() {
        assert_eq!(scrape_credits(0, true, true), 10);
    }

    #[test]
    fn test_scrape_credits_combined() {
        // 2 feature formats + AI summary + AI extraction = 2 + 5 + 5 = 12
        assert_eq!(scrape_credits(2, true, true), 12);
    }

    #[test]
    fn test_crawl_credits_http_no_features() {
        let features = FeaturesConfig::default();
        assert_eq!(crawl_credits_per_page(&CrawlerType::Http, &features), 1);
    }

    #[test]
    fn test_crawl_credits_browser_base() {
        let features = FeaturesConfig::default();
        assert_eq!(crawl_credits_per_page(&CrawlerType::Browser, &features), 2);
    }

    #[test]
    fn test_crawl_credits_with_features() {
        let features = FeaturesConfig::from_cli_args(true, true, true, true, false, false, None);
        // 1 base + 4 features = 5
        assert_eq!(crawl_credits_per_page(&CrawlerType::Http, &features), 5);
    }

    #[test]
    fn test_crawl_credits_with_ai() {
        let features = FeaturesConfig::from_cli_args(
            false,
            false,
            false,
            false,
            true,
            true,
            Some("extract product info".to_string()),
        );
        // 1 base + 5 + 5 = 11
        assert_eq!(crawl_credits_per_page(&CrawlerType::Http, &features), 11);
    }

    #[test]
    fn test_crawl_credits_browser_all_features() {
        let features = FeaturesConfig::from_cli_args(
            true,
            true,
            true,
            true,
            true,
            true,
            Some("extract".to_string()),
        );
        // 2 base + 4 features + 5 ai_extraction + 5 ai_summary = 16
        assert_eq!(crawl_credits_per_page(&CrawlerType::Browser, &features), 16);
    }

    #[test]
    fn test_map_credits_constant() {
        assert_eq!(MAP_CREDITS, 2);
    }
}
