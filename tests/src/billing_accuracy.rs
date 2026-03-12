//! Billing Accuracy Tests (P0)
//!
//! content_length and account_id flow through the pipeline for billing.
//! If content_length is wrong, customers are over/under-charged.
//! If account_id is lost, usage can't be attributed.

use scrapix_core::{Account, BillingTier, CrawlUrl, UsageMetrics};
use scrapix_queue::{CrawlEvent, RawPageMessage, UrlMessage};

// ============================================================================
// Content length tracking through messages
// ============================================================================

#[test]
fn test_content_length_propagates_in_raw_page_message() {
    let html = "<html><body><p>Hello World</p></body></html>";
    let content_length = html.len() as u64;

    let msg = RawPageMessage {
        url: "https://example.com".to_string(),
        final_url: "https://example.com".to_string(),
        status: 200,
        html: html.to_string(),
        content_type: Some("text/html".to_string()),
        content_length,
        js_rendered: false,
        fetched_at: 1704067200000,
        fetch_duration_ms: 100,
        job_id: "job-1".to_string(),
        index_uid: "idx".to_string(),
        account_id: Some("acct_123".to_string()),
        source: None,
        message_id: "msg-1".to_string(),
        etag: None,
        last_modified: None,
        meilisearch_url: None,
        meilisearch_api_key: None,
        features: None,
    };

    let json = serde_json::to_string(&msg).unwrap();
    let d: RawPageMessage = serde_json::from_str(&json).unwrap();

    assert_eq!(d.content_length, content_length);
    assert_eq!(d.content_length, html.len() as u64);
}

#[test]
fn test_content_length_zero_default_in_raw_page_message() {
    // Older messages without content_length field should default to 0
    let json = r#"{
        "url": "https://example.com",
        "final_url": "https://example.com",
        "status": 200,
        "html": "<html></html>",
        "content_type": null,
        "js_rendered": false,
        "fetched_at": 1704067200000,
        "fetch_duration_ms": 100,
        "job_id": "j",
        "index_uid": "i",
        "message_id": "m"
    }"#;

    let msg: RawPageMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.content_length, 0);
}

#[test]
fn test_content_length_in_crawl_event() {
    let event = CrawlEvent::page_crawled_with_billing(
        "job-1",
        Some("acct_123".to_string()),
        "https://example.com",
        200,
        54321,
        150,
    );

    let json = serde_json::to_string(&event).unwrap();
    let d: CrawlEvent = serde_json::from_str(&json).unwrap();

    match d {
        CrawlEvent::PageCrawled { content_length, .. } => {
            assert_eq!(content_length, 54321);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_content_length_zero_in_page_crawled_without_billing() {
    // Non-billing event defaults content_length to 0
    let event = CrawlEvent::page_crawled("job-1", "https://example.com", 200, 150);

    match event {
        CrawlEvent::PageCrawled { content_length, .. } => {
            assert_eq!(content_length, 0);
        }
        _ => panic!("Wrong variant"),
    }
}

// ============================================================================
// Account ID propagation
// ============================================================================

#[test]
fn test_account_id_propagates_through_url_message() {
    let msg = UrlMessage::with_account(
        CrawlUrl::seed("https://example.com"),
        "job-1",
        "idx",
        "acct_billing",
    );

    assert_eq!(msg.account_id, Some("acct_billing".to_string()));

    let json = serde_json::to_string(&msg).unwrap();
    let d: UrlMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(d.account_id, Some("acct_billing".to_string()));
}

#[test]
fn test_account_id_none_for_anonymous_crawls() {
    let msg = UrlMessage::new(CrawlUrl::seed("https://example.com"), "job-1", "idx");

    assert!(msg.account_id.is_none());

    let json = serde_json::to_string(&msg).unwrap();
    let d: UrlMessage = serde_json::from_str(&json).unwrap();
    assert!(d.account_id.is_none());
}

#[test]
fn test_account_id_in_job_started_event() {
    let event = CrawlEvent::job_started_with_account(
        "job-1",
        "index-1",
        "acct_123",
        vec!["https://example.com".to_string()],
    );

    match event {
        CrawlEvent::JobStarted { account_id, .. } => {
            assert_eq!(account_id, Some("acct_123".to_string()));
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_account_id_none_in_events_without_account() {
    let event = CrawlEvent::page_crawled("job-1", "https://example.com", 200, 100);

    match event {
        CrawlEvent::PageCrawled { account_id, .. } => {
            assert!(account_id.is_none());
        }
        _ => panic!("Wrong variant"),
    }
}

// ============================================================================
// Billing tier limits and usage tracking
// ============================================================================

#[test]
fn test_billing_tier_rate_limits_ordered() {
    // Higher tiers should have higher limits
    assert!(BillingTier::Starter.rate_limit() > BillingTier::Free.rate_limit());
    assert!(BillingTier::Pro.rate_limit() > BillingTier::Starter.rate_limit());
    assert!(BillingTier::Enterprise.rate_limit() > BillingTier::Pro.rate_limit());
}

#[test]
fn test_billing_tier_quotas_ordered() {
    assert!(BillingTier::Starter.monthly_quota() > BillingTier::Free.monthly_quota());
    assert!(BillingTier::Pro.monthly_quota() > BillingTier::Starter.monthly_quota());
    assert!(BillingTier::Enterprise.monthly_quota() > BillingTier::Pro.monthly_quota());
}

#[test]
fn test_billing_tier_bandwidth_quotas_ordered() {
    assert!(BillingTier::Starter.bandwidth_quota() > BillingTier::Free.bandwidth_quota());
    assert!(BillingTier::Pro.bandwidth_quota() > BillingTier::Starter.bandwidth_quota());
    assert!(BillingTier::Enterprise.bandwidth_quota() > BillingTier::Pro.bandwidth_quota());
}

#[test]
fn test_billing_tier_prices_decreasing() {
    // Higher tiers should have lower per-unit pricing
    assert!(BillingTier::Pro.price_per_1k_pages() < BillingTier::Starter.price_per_1k_pages());
    assert!(BillingTier::Enterprise.price_per_1k_pages() < BillingTier::Pro.price_per_1k_pages());
    assert_eq!(BillingTier::Free.price_per_1k_pages(), 0);
}

#[test]
fn test_usage_exceeds_quota_by_pages() {
    let account = Account::new("acct_1", "Test", BillingTier::Free);
    let mut usage = UsageMetrics::new("acct_1", "2024-01-01", "2024-02-01");

    usage.pages_crawled = 999;
    assert!(!usage.exceeds_quota(&account));

    usage.pages_crawled = 1000; // == quota
    assert!(usage.exceeds_quota(&account));
}

#[test]
fn test_usage_exceeds_quota_by_bandwidth() {
    let account = Account::new("acct_1", "Test", BillingTier::Free);
    let mut usage = UsageMetrics::new("acct_1", "2024-01-01", "2024-02-01");

    // Free tier has 100MB bandwidth quota
    usage.bytes_downloaded = 99 * 1024 * 1024;
    assert!(!usage.exceeds_quota(&account));

    usage.bytes_downloaded = 100 * 1024 * 1024; // == quota
    assert!(usage.exceeds_quota(&account));
}

#[test]
fn test_usage_cost_calculation() {
    let mut usage = UsageMetrics::new("acct_1", "2024-01-01", "2024-02-01");
    usage.pages_crawled = 10_000;

    // Starter: $1.00 per 1000 pages = 100 cents per 1000
    let cost = usage.estimated_cost(BillingTier::Starter);
    assert_eq!(cost, 1000); // 10 * 100 cents = $10.00

    // Pro: $0.50 per 1000 = 50 cents per 1000
    let cost = usage.estimated_cost(BillingTier::Pro);
    assert_eq!(cost, 500); // 10 * 50 = $5.00

    // Free: $0
    let cost = usage.estimated_cost(BillingTier::Free);
    assert_eq!(cost, 0);
}

#[test]
fn test_account_override_takes_precedence() {
    let mut account = Account::new("acct_1", "VIP", BillingTier::Free);
    account.rate_limit_override = Some(500);
    account.quota_override = Some(1_000_000);

    // Overrides should take precedence over tier defaults
    assert_eq!(account.rate_limit(), 500); // not Free's 10
    assert_eq!(account.monthly_quota(), 1_000_000); // not Free's 1000
}

#[test]
fn test_js_rendering_tier_gating() {
    assert!(!BillingTier::Free.js_rendering_enabled());
    assert!(!BillingTier::Starter.js_rendering_enabled());
    assert!(BillingTier::Pro.js_rendering_enabled());
    assert!(BillingTier::Enterprise.js_rendering_enabled());
}

#[test]
fn test_quota_percentage_calculation() {
    let account = Account::new("acct_1", "Test", BillingTier::Free);
    let mut usage = UsageMetrics::new("acct_1", "2024-01-01", "2024-02-01");

    // Free tier: 1000 pages, 100MB bandwidth
    usage.pages_crawled = 500;
    usage.bytes_downloaded = 50 * 1024 * 1024;

    let pct = usage.quota_percentage(&account);
    assert_eq!(pct, 50.0); // Both at 50%, returns max
}
