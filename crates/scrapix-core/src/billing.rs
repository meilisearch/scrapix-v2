//! # Billing Types
//!
//! Types for account management, API key authentication, and usage tracking.
//!
//! ## Overview
//!
//! - `Account` - Billable entity with quota and tier configuration
//! - `ApiKey` - Authentication token linked to an account
//! - `UsageMetrics` - Per-period usage tracking for billing
//! - `BillingTier` - Plan definitions with limits
//!
//! ## Example
//!
//! ```rust,ignore
//! use scrapix_core::billing::{Account, ApiKey, BillingTier};
//!
//! let account = Account::new("acct_123", "ACME Corp", BillingTier::Pro);
//! let api_key = ApiKey::generate(&account.id);
//! ```

use serde::{Deserialize, Serialize};

// ============================================================================
// Account
// ============================================================================

/// Unique account identifier (e.g., "acct_abc123")
pub type AccountId = String;

/// Unique API key identifier (e.g., "key_xyz789")
pub type ApiKeyId = String;

/// A billable account/tenant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Unique account identifier
    pub id: AccountId,

    /// Display name
    pub name: String,

    /// Email for billing notifications
    #[serde(default)]
    pub email: Option<String>,

    /// Billing tier/plan
    pub tier: BillingTier,

    /// Whether the account is active
    #[serde(default = "default_true")]
    pub active: bool,

    /// Account creation timestamp (RFC3339)
    pub created_at: String,

    /// Custom rate limit override (requests per minute)
    #[serde(default)]
    pub rate_limit_override: Option<u32>,

    /// Custom quota override (pages per month)
    #[serde(default)]
    pub quota_override: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl Account {
    /// Create a new account
    pub fn new(id: impl Into<String>, name: impl Into<String>, tier: BillingTier) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            email: None,
            tier,
            active: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            rate_limit_override: None,
            quota_override: None,
        }
    }

    /// Get effective rate limit (override or tier default)
    pub fn rate_limit(&self) -> u32 {
        self.rate_limit_override
            .unwrap_or_else(|| self.tier.rate_limit())
    }

    /// Get effective monthly quota (override or tier default)
    pub fn monthly_quota(&self) -> u64 {
        self.quota_override
            .unwrap_or_else(|| self.tier.monthly_quota())
    }

    /// Get effective monthly bandwidth quota in bytes
    pub fn bandwidth_quota(&self) -> u64 {
        self.tier.bandwidth_quota()
    }
}

// ============================================================================
// API Key
// ============================================================================

/// An API key for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique key identifier (for reference, not the actual key)
    pub id: ApiKeyId,

    /// The actual API key value (hashed in storage, shown once on creation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// Account this key belongs to
    pub account_id: AccountId,

    /// Human-readable name/description
    pub name: String,

    /// Key prefix for identification (e.g., "sk_live_abc...")
    pub prefix: String,

    /// Whether the key is active
    #[serde(default = "default_true")]
    pub active: bool,

    /// Creation timestamp
    pub created_at: String,

    /// Last used timestamp
    #[serde(default)]
    pub last_used_at: Option<String>,

    /// Optional expiration timestamp
    #[serde(default)]
    pub expires_at: Option<String>,

    /// Scopes/permissions (empty = full access)
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl ApiKey {
    /// Generate a new API key for an account
    pub fn generate(account_id: impl Into<String>, name: impl Into<String>) -> Self {
        let key_id = format!("key_{}", uuid::Uuid::new_v4().simple());
        let key_value = format!("sk_live_{}", uuid::Uuid::new_v4().simple());
        let prefix = key_value.chars().take(12).collect();

        Self {
            id: key_id,
            key: Some(key_value),
            account_id: account_id.into(),
            name: name.into(),
            prefix,
            active: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_used_at: None,
            expires_at: None,
            scopes: vec![],
        }
    }

    /// Create a key reference (without the actual key value)
    pub fn as_reference(&self) -> Self {
        Self {
            key: None,
            ..self.clone()
        }
    }
}

// ============================================================================
// Billing Tier
// ============================================================================

/// Billing tier/plan definition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BillingTier {
    /// Free tier with limited usage
    #[default]
    Free,

    /// Starter tier for small projects
    Starter,

    /// Pro tier for production use
    Pro,

    /// Enterprise tier with custom limits
    Enterprise,
}

impl BillingTier {
    /// Requests per minute limit
    pub fn rate_limit(&self) -> u32 {
        match self {
            BillingTier::Free => 10,
            BillingTier::Starter => 60,
            BillingTier::Pro => 300,
            BillingTier::Enterprise => 1000,
        }
    }

    /// Monthly page crawl quota
    pub fn monthly_quota(&self) -> u64 {
        match self {
            BillingTier::Free => 1_000,
            BillingTier::Starter => 50_000,
            BillingTier::Pro => 500_000,
            BillingTier::Enterprise => 10_000_000,
        }
    }

    /// Monthly bandwidth quota in bytes
    pub fn bandwidth_quota(&self) -> u64 {
        match self {
            BillingTier::Free => 100 * 1024 * 1024,              // 100 MB
            BillingTier::Starter => 5 * 1024 * 1024 * 1024,      // 5 GB
            BillingTier::Pro => 50 * 1024 * 1024 * 1024,         // 50 GB
            BillingTier::Enterprise => 500 * 1024 * 1024 * 1024, // 500 GB
        }
    }

    /// Maximum concurrent jobs
    pub fn max_concurrent_jobs(&self) -> u32 {
        match self {
            BillingTier::Free => 1,
            BillingTier::Starter => 3,
            BillingTier::Pro => 10,
            BillingTier::Enterprise => 100,
        }
    }

    /// Maximum depth per crawl
    pub fn max_depth(&self) -> u32 {
        match self {
            BillingTier::Free => 2,
            BillingTier::Starter => 5,
            BillingTier::Pro => 20,
            BillingTier::Enterprise => 100,
        }
    }

    /// Whether JS rendering is available
    pub fn js_rendering_enabled(&self) -> bool {
        match self {
            BillingTier::Free => false,
            BillingTier::Starter => false,
            BillingTier::Pro => true,
            BillingTier::Enterprise => true,
        }
    }

    /// Price per 1000 pages (in cents)
    pub fn price_per_1k_pages(&self) -> u32 {
        match self {
            BillingTier::Free => 0,
            BillingTier::Starter => 100,   // $1.00 per 1k
            BillingTier::Pro => 50,        // $0.50 per 1k
            BillingTier::Enterprise => 25, // $0.25 per 1k
        }
    }
}

impl std::str::FromStr for BillingTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "free" => Ok(Self::Free),
            "starter" => Ok(Self::Starter),
            "pro" => Ok(Self::Pro),
            "enterprise" => Ok(Self::Enterprise),
            other => Err(format!("Unknown billing tier: {other}")),
        }
    }
}

impl std::fmt::Display for BillingTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BillingTier::Free => write!(f, "Free"),
            BillingTier::Starter => write!(f, "Starter"),
            BillingTier::Pro => write!(f, "Pro"),
            BillingTier::Enterprise => write!(f, "Enterprise"),
        }
    }
}

// ============================================================================
// Usage Metrics
// ============================================================================

/// Usage metrics for a billing period
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageMetrics {
    /// Account ID
    pub account_id: AccountId,

    /// Billing period start (RFC3339)
    pub period_start: String,

    /// Billing period end (RFC3339)
    pub period_end: String,

    /// Total pages crawled
    pub pages_crawled: u64,

    /// Total bytes downloaded
    pub bytes_downloaded: u64,

    /// Total API requests made
    pub api_requests: u64,

    /// Number of jobs created
    pub jobs_created: u64,

    /// Number of successful crawls
    pub successful_crawls: u64,

    /// Number of failed crawls
    pub failed_crawls: u64,

    /// JS rendering requests
    pub js_renders: u64,

    /// Unique domains crawled
    pub unique_domains: u64,

    /// Total documents indexed
    pub documents_indexed: u64,
}

impl UsageMetrics {
    /// Create new usage metrics for a period
    pub fn new(account_id: impl Into<String>, period_start: &str, period_end: &str) -> Self {
        Self {
            account_id: account_id.into(),
            period_start: period_start.to_string(),
            period_end: period_end.to_string(),
            ..Default::default()
        }
    }

    /// Calculate estimated cost in cents based on tier pricing
    pub fn estimated_cost(&self, tier: BillingTier) -> u64 {
        (self.pages_crawled / 1000) * tier.price_per_1k_pages() as u64
    }

    /// Check if usage exceeds quota
    pub fn exceeds_quota(&self, account: &Account) -> bool {
        self.pages_crawled >= account.monthly_quota()
            || self.bytes_downloaded >= account.bandwidth_quota()
    }

    /// Get quota usage percentage (0-100+)
    pub fn quota_percentage(&self, account: &Account) -> f64 {
        let page_pct = (self.pages_crawled as f64 / account.monthly_quota() as f64) * 100.0;
        let bw_pct = (self.bytes_downloaded as f64 / account.bandwidth_quota() as f64) * 100.0;
        page_pct.max(bw_pct)
    }
}

// ============================================================================
// Rate Limit Info
// ============================================================================

/// Rate limit status for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Maximum requests per window
    pub limit: u32,

    /// Remaining requests in current window
    pub remaining: u32,

    /// Window reset timestamp (Unix seconds)
    pub reset_at: i64,

    /// Whether currently rate limited
    pub limited: bool,
}

impl RateLimitInfo {
    /// Create rate limit info
    pub fn new(limit: u32, remaining: u32, reset_at: i64) -> Self {
        Self {
            limit,
            remaining,
            reset_at,
            limited: remaining == 0,
        }
    }

    /// Seconds until reset
    pub fn retry_after(&self) -> i64 {
        let now = chrono::Utc::now().timestamp();
        (self.reset_at - now).max(0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::new("acct_123", "Test Account", BillingTier::Pro);
        assert_eq!(account.id, "acct_123");
        assert_eq!(account.tier, BillingTier::Pro);
        assert!(account.active);
        assert_eq!(account.rate_limit(), 300);
        assert_eq!(account.monthly_quota(), 500_000);
    }

    #[test]
    fn test_account_overrides() {
        let mut account = Account::new("acct_123", "Test", BillingTier::Free);
        account.rate_limit_override = Some(100);
        account.quota_override = Some(10_000);

        assert_eq!(account.rate_limit(), 100);
        assert_eq!(account.monthly_quota(), 10_000);
    }

    #[test]
    fn test_api_key_generation() {
        let key = ApiKey::generate("acct_123", "Production Key");
        assert!(key.key.is_some());
        assert!(key.key.as_ref().unwrap().starts_with("sk_live_"));
        assert_eq!(key.account_id, "acct_123");
        assert!(key.active);
    }

    #[test]
    fn test_api_key_reference() {
        let key = ApiKey::generate("acct_123", "Test");
        let reference = key.as_reference();
        assert!(reference.key.is_none());
        assert_eq!(reference.id, key.id);
    }

    #[test]
    fn test_billing_tier_limits() {
        assert_eq!(BillingTier::Free.rate_limit(), 10);
        assert_eq!(BillingTier::Pro.monthly_quota(), 500_000);
        assert!(!BillingTier::Starter.js_rendering_enabled());
        assert!(BillingTier::Enterprise.js_rendering_enabled());
    }

    #[test]
    fn test_usage_metrics_quota_check() {
        let account = Account::new("acct_123", "Test", BillingTier::Free);
        let mut usage = UsageMetrics::new("acct_123", "2024-01-01", "2024-02-01");

        usage.pages_crawled = 500;
        assert!(!usage.exceeds_quota(&account));

        usage.pages_crawled = 1500;
        assert!(usage.exceeds_quota(&account));
    }

    #[test]
    fn test_billing_tier_from_str() {
        assert_eq!("free".parse::<BillingTier>().unwrap(), BillingTier::Free);
        assert_eq!("Pro".parse::<BillingTier>().unwrap(), BillingTier::Pro);
        assert_eq!(
            "ENTERPRISE".parse::<BillingTier>().unwrap(),
            BillingTier::Enterprise
        );
        assert!("invalid".parse::<BillingTier>().is_err());
    }

    #[test]
    fn test_rate_limit_info() {
        let info = RateLimitInfo::new(100, 50, chrono::Utc::now().timestamp() + 60);
        assert!(!info.limited);
        assert_eq!(info.limit, 100);
        assert!(info.retry_after() <= 60);
    }
}
