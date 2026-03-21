//! Credit consumption and billing logic.
//!
//! All credit operations are centralized here: balance checks, atomic
//! deductions, auto-topup, and spend-limit enforcement.

use sqlx::Row;
use tracing::{debug, error, info, warn};

use crate::email::EmailClient;
use crate::{ApiError, ScrapeFormat};
use scrapix_core::{CrawlerType, FeaturesConfig};

// ============================================================================
// Public API
// ============================================================================

/// Check that the account has at least `required_amount` credits.
/// Returns the current balance on success, or a 402 Payment Required error.
pub(crate) async fn check_credits(
    pool: &sqlx::PgPool,
    account_id: &str,
    required_amount: i64,
) -> Result<i64, ApiError> {
    let account_uuid = parse_uuid(account_id)?;

    let balance: i64 =
        sqlx::query_scalar("SELECT credits_balance FROM accounts WHERE id = $1 AND active = true")
            .bind(account_uuid)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to query credits balance");
                ApiError::new("Database error", "internal_error")
            })?
            .ok_or_else(|| ApiError::new("Account not found or inactive", "not_found"))?;

    if balance < required_amount {
        return Err(ApiError::new(
            format!(
                "Insufficient credits: {} available, {} required",
                balance, required_amount
            ),
            "insufficient_credits",
        ));
    }

    Ok(balance)
}

/// Atomically deduct credits from an account and record a transaction.
///
/// Uses `UPDATE ... WHERE credits_balance >= amount RETURNING credits_balance`
/// so the deduction only succeeds if the account has enough credits.
/// Returns the new balance on success.
pub(crate) async fn deduct_credits(
    pool: &sqlx::PgPool,
    account_id: &str,
    amount: i64,
    operation: &str,
    description: &str,
) -> Result<i64, ApiError> {
    let account_uuid = parse_uuid(account_id)?;

    let mut tx = pool.begin().await.map_err(|e| {
        error!(error = %e, "Failed to begin transaction");
        ApiError::new("Database error", "internal_error")
    })?;

    // Atomic check-and-deduct
    let new_balance: Option<i64> = sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance - $1 \
         WHERE id = $2 AND credits_balance >= $1 \
         RETURNING credits_balance",
    )
    .bind(amount)
    .bind(account_uuid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to deduct credits");
        ApiError::new("Database error", "internal_error")
    })?;

    let new_balance = match new_balance {
        Some(b) => b,
        None => {
            // Either account doesn't exist or insufficient balance
            return Err(ApiError::new(
                "Insufficient credits",
                "insufficient_credits",
            ));
        }
    };

    // Record the transaction
    let desc = format!("{}: {}", operation, description);
    sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'usage_deduction', $2, $3, $4)",
    )
    .bind(account_uuid)
    .bind(-amount) // negative amount for deductions
    .bind(new_balance)
    .bind(&desc)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to insert deduction transaction");
        ApiError::new("Database error", "internal_error")
    })?;

    tx.commit().await.map_err(|e| {
        error!(error = %e, "Failed to commit deduction transaction");
        ApiError::new("Database error", "internal_error")
    })?;

    debug!(
        account_id = %account_id,
        amount,
        new_balance,
        operation,
        "Credits deducted"
    );

    Ok(new_balance)
}

/// Combined atomic check-and-deduct, with auto-topup triggered synchronously
/// (with timeout) if the balance drops below the threshold.
///
/// Returns the new balance on success.
pub(crate) async fn check_credits_and_deduct(
    pool: &sqlx::PgPool,
    account_id: &str,
    amount: i64,
    operation: &str,
    description: &str,
    email_client: Option<&EmailClient>,
) -> Result<i64, ApiError> {
    let new_balance = deduct_credits(pool, account_id, amount, operation, description).await?;

    // Synchronous auto-topup with timeout to prevent blocking on DB issues
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        maybe_auto_topup(pool, account_id, None, email_client),
    )
    .await
    {
        Ok(()) => {}
        Err(_) => {
            warn!(account_id, "Auto-topup timed out after 5s");
        }
    }

    // Send low balance warning if below threshold (10 credits)
    if new_balance > 0 && new_balance <= 10 {
        if let Some(mailer) = email_client {
            let account_uuid = uuid::Uuid::parse_str(account_id).ok();
            if let Some(uuid) = account_uuid {
                if let Some(email) = crate::email::get_account_email(pool, uuid).await {
                    mailer.send_low_balance_warning(&email, new_balance);
                }
            }
        }
    }

    Ok(new_balance)
}

/// Check if the account's balance has dropped below its auto-topup threshold,
/// and if so, top up. When a Stripe client is provided and the account has a
/// saved payment method, the auto-topup charges the card via Stripe. Otherwise
/// falls back to the free (no-charge) top-up for dev/test.
pub(crate) async fn maybe_auto_topup(
    pool: &sqlx::PgPool,
    account_id: &str,
    stripe_client: Option<&stripe::Client>,
    email_client: Option<&EmailClient>,
) {
    let account_uuid = match uuid::Uuid::parse_str(account_id) {
        Ok(u) => u,
        Err(_) => return,
    };

    // Read account settings
    let row = match sqlx::query(
        "SELECT credits_balance, auto_topup_enabled, auto_topup_amount, auto_topup_threshold, \
         stripe_customer_id, stripe_default_payment_method_id \
         FROM accounts WHERE id = $1",
    )
    .bind(account_uuid)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(e) => {
            warn!(error = %e, account_id, "auto-topup: failed to read account");
            return;
        }
    };

    let enabled: bool = row.get("auto_topup_enabled");
    if !enabled {
        return;
    }

    let balance: i64 = row.get("credits_balance");
    let threshold: i64 = row.get("auto_topup_threshold");
    let topup_amount: i64 = row.get("auto_topup_amount");

    if balance >= threshold {
        return;
    }

    // Check spend limit before auto-topup
    if let Err(e) = check_spend_limit(pool, account_uuid, topup_amount).await {
        warn!(
            account_id,
            "auto-topup skipped: spend limit would be exceeded: {:?}", e
        );
        return;
    }

    // Try Stripe-based auto-topup if available
    let has_stripe_pm: bool = row
        .get::<Option<String>, _>("stripe_default_payment_method_id")
        .is_some();

    if let Some(stripe) = stripe_client {
        if has_stripe_pm {
            match crate::stripe::charge_auto_topup(stripe, pool, account_uuid, topup_amount).await {
                Ok(()) => {
                    info!(account_id, topup_amount, "Auto top-up via Stripe completed");
                    // Send auto-topup receipt email
                    if let Some(mailer) = email_client {
                        let amount_cents = crate::stripe::calculate_price_cents(topup_amount);
                        let new_bal: i64 = sqlx::query_scalar(
                            "SELECT credits_balance FROM accounts WHERE id = $1",
                        )
                        .bind(account_uuid)
                        .fetch_optional(pool)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0);
                        if let Some(email) =
                            crate::email::get_account_email(pool, account_uuid).await
                        {
                            mailer.send_auto_topup_receipt(
                                &email,
                                topup_amount,
                                amount_cents,
                                new_bal,
                            );
                        }
                    }
                    return;
                }
                Err(e) => {
                    warn!(account_id, error = %e, "Stripe auto-topup failed, falling back to free topup");
                    // Send auto-topup failure email
                    if let Some(mailer) = email_client {
                        if let Some(email) =
                            crate::email::get_account_email(pool, account_uuid).await
                        {
                            mailer.send_auto_topup_failed(&email, &e);
                        }
                    }
                }
            }
        }
    }

    // Fallback: free top-up (no payment — for dev/test or accounts without a card)
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            warn!(error = %e, account_id, "auto-topup: failed to begin transaction");
            return;
        }
    };

    let new_balance: i64 = match sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance + $1 WHERE id = $2 RETURNING credits_balance",
    )
    .bind(topup_amount)
    .bind(account_uuid)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(b) => b,
        Err(e) => {
            warn!(error = %e, account_id, "auto-topup: failed to update balance");
            return;
        }
    };

    if let Err(e) = sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'auto_topup', $2, $3, 'Automatic credit top-up')",
    )
    .bind(account_uuid)
    .bind(topup_amount)
    .bind(new_balance)
    .execute(&mut *tx)
    .await
    {
        warn!(error = %e, account_id, "auto-topup: failed to insert transaction");
        return;
    }

    if let Err(e) = tx.commit().await {
        warn!(error = %e, account_id, "auto-topup: failed to commit");
        return;
    }

    info!(
        account_id,
        topup_amount, new_balance, "Auto top-up completed (free)"
    );
}

/// Check if a top-up amount would exceed the monthly spend limit.
///
/// This is moved from `auth/handlers.rs` and made public so both the manual
/// top-up handler and the auto-topup logic can share it.
pub(crate) async fn check_spend_limit(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    amount: i64,
) -> Result<(), ApiError> {
    let row = sqlx::query("SELECT monthly_spend_limit FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to query spend limit");
            ApiError::new("Database error", "internal_error")
        })?
        .ok_or_else(|| ApiError::new("Account not found", "not_found"))?;

    let limit: Option<i64> = row.get("monthly_spend_limit");
    if let Some(limit) = limit {
        // Sum all top-ups this calendar month
        let spent: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0)::BIGINT FROM transactions \
             WHERE account_id = $1 \
             AND type IN ('manual_topup', 'auto_topup') \
             AND created_at >= date_trunc('month', now())",
        )
        .bind(account_id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to query monthly spend");
            ApiError::new("Database error", "internal_error")
        })?;

        if spent + amount > limit {
            return Err(ApiError::new(
                "Monthly spend limit reached",
                "spend_limit_exceeded",
            ));
        }
    }

    Ok(())
}

// ============================================================================
// Credit calculation
// ============================================================================

/// Compute credits for a /scrape request.
///
/// - Base formats (Html, RawHtml, Content) are free
/// - Feature formats (Markdown, Links, Metadata, Screenshot, Schema, Blocks): +1 each
/// - AI summary: +5
/// - AI extraction: +5
/// - Minimum: 1 credit (even if no features)
pub(crate) fn scrape_credits(
    formats: &[ScrapeFormat],
    has_ai_summary: bool,
    has_ai_extraction: bool,
) -> i64 {
    let feature_count = formats
        .iter()
        .filter(|f| {
            matches!(
                f,
                ScrapeFormat::Markdown
                    | ScrapeFormat::Links
                    | ScrapeFormat::Metadata
                    | ScrapeFormat::Screenshot
                    | ScrapeFormat::Schema
                    | ScrapeFormat::Blocks
            )
        })
        .count() as i64;

    let ai_cost = if has_ai_summary { 5 } else { 0 } + if has_ai_extraction { 5 } else { 0 };

    // At least 1 credit per scrape
    (feature_count + ai_cost).max(1)
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

// ============================================================================
// Helpers
// ============================================================================

fn parse_uuid(id: &str) -> Result<uuid::Uuid, ApiError> {
    uuid::Uuid::parse_str(id).map_err(|_| ApiError::new("Invalid account ID", "internal_error"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrape_credits_minimum_one() {
        // Even with no feature formats, minimum is 1 credit
        let credits = scrape_credits(&[], false, false);
        assert_eq!(credits, 1);
    }

    #[test]
    fn test_scrape_credits_base_formats_free() {
        // Html, RawHtml, Content are free (still minimum 1)
        let credits = scrape_credits(
            &[
                ScrapeFormat::Html,
                ScrapeFormat::RawHtml,
                ScrapeFormat::Content,
            ],
            false,
            false,
        );
        assert_eq!(credits, 1);
    }

    #[test]
    fn test_scrape_credits_feature_formats() {
        // Each feature format costs 1 credit
        let credits = scrape_credits(&[ScrapeFormat::Markdown], false, false);
        assert_eq!(credits, 1);

        let credits = scrape_credits(
            &[
                ScrapeFormat::Markdown,
                ScrapeFormat::Links,
                ScrapeFormat::Metadata,
            ],
            false,
            false,
        );
        assert_eq!(credits, 3);

        let credits = scrape_credits(
            &[
                ScrapeFormat::Markdown,
                ScrapeFormat::Links,
                ScrapeFormat::Metadata,
                ScrapeFormat::Screenshot,
                ScrapeFormat::Schema,
                ScrapeFormat::Blocks,
            ],
            false,
            false,
        );
        assert_eq!(credits, 6);
    }

    #[test]
    fn test_scrape_credits_ai_summary() {
        let credits = scrape_credits(&[], true, false);
        assert_eq!(credits, 5);
    }

    #[test]
    fn test_scrape_credits_ai_extraction() {
        let credits = scrape_credits(&[], false, true);
        assert_eq!(credits, 5);
    }

    #[test]
    fn test_scrape_credits_ai_both() {
        let credits = scrape_credits(&[], true, true);
        assert_eq!(credits, 10);
    }

    #[test]
    fn test_scrape_credits_combined() {
        // 2 feature formats + AI summary + AI extraction = 2 + 5 + 5 = 12
        let credits = scrape_credits(&[ScrapeFormat::Markdown, ScrapeFormat::Schema], true, true);
        assert_eq!(credits, 12);
    }

    #[test]
    fn test_scrape_credits_mixed_base_and_feature() {
        // Base (Html) is free, only Markdown counts = 1
        let credits = scrape_credits(&[ScrapeFormat::Html, ScrapeFormat::Markdown], false, false);
        assert_eq!(credits, 1);
    }

    #[test]
    fn test_crawl_credits_http_no_features() {
        let features = FeaturesConfig::default();
        let credits = crawl_credits_per_page(&CrawlerType::Http, &features);
        assert_eq!(credits, 1);
    }

    #[test]
    fn test_crawl_credits_browser_base() {
        let features = FeaturesConfig::default();
        let credits = crawl_credits_per_page(&CrawlerType::Browser, &features);
        assert_eq!(credits, 2);
    }

    #[test]
    fn test_crawl_credits_with_features() {
        let features = FeaturesConfig::from_cli_args(
            true,  // metadata +1
            true,  // markdown +1
            true,  // schema +1
            true,  // block_split +1
            false, // ai_summary
            false, // ai_extraction
            None,
        );
        let credits = crawl_credits_per_page(&CrawlerType::Http, &features);
        // 1 base + 4 features = 5
        assert_eq!(credits, 5);
    }

    #[test]
    fn test_crawl_credits_with_ai() {
        let features = FeaturesConfig::from_cli_args(
            false,
            false,
            false,
            false,
            true, // ai_summary +5
            true, // ai_extraction +5
            Some("extract product info".to_string()),
        );
        let credits = crawl_credits_per_page(&CrawlerType::Http, &features);
        // 1 base + 5 + 5 = 11
        assert_eq!(credits, 11);
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
        let credits = crawl_credits_per_page(&CrawlerType::Browser, &features);
        // 2 base + 4 features + 5 ai_extraction + 5 ai_summary = 16
        assert_eq!(credits, 16);
    }

    #[test]
    fn test_map_credits_constant() {
        assert_eq!(MAP_CREDITS, 2);
    }
}
