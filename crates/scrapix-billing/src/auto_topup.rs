//! Auto top-up logic.
//!
//! When an account's credit balance drops below its configured threshold,
//! this module attempts to charge the saved payment method (via [`PaymentProvider`])
//! and notifies the account owner (via [`BillingNotifier`]).

use sqlx::Row;
use tracing::{info, warn};

use crate::error::BillingError;
use crate::ledger::check_spend_limit;
use crate::pricing::calculate_price_cents;

// ============================================================================
// Traits
// ============================================================================

/// Abstraction over a payment backend (Stripe, test stub, etc.).
#[async_trait::async_trait]
pub trait PaymentProvider: Send + Sync {
    /// Charge the account's saved payment method for `credits`.
    /// Returns the payment intent ID on success.
    async fn charge_auto_topup(
        &self,
        pool: &sqlx::PgPool,
        account_id: uuid::Uuid,
        credits: i64,
    ) -> Result<String, String>;
}

/// Abstraction over notification delivery (email, webhook, etc.).
pub trait BillingNotifier: Send + Sync {
    /// Notify the account owner that an auto top-up was processed.
    fn notify_auto_topup_success(
        &self,
        pool: &sqlx::PgPool,
        account_id: uuid::Uuid,
        credits: i64,
        amount_cents: i64,
        new_balance: i64,
    );

    /// Notify the account owner that an auto top-up failed.
    fn notify_auto_topup_failure(&self, pool: &sqlx::PgPool, account_id: uuid::Uuid, reason: &str);

    /// Notify the account owner that their balance is low.
    fn notify_low_balance(&self, pool: &sqlx::PgPool, account_id: uuid::Uuid, balance: i64);
}

// ============================================================================
// Combined check-deduct-topup
// ============================================================================

/// Combined atomic check-and-deduct, with auto-topup triggered synchronously
/// (with timeout) if the balance drops below the threshold.
///
/// Returns the new balance on success.
pub async fn check_credits_and_deduct(
    pool: &sqlx::PgPool,
    account_id: &str,
    amount: i64,
    operation: &str,
    description: &str,
    payment_provider: Option<&dyn PaymentProvider>,
    notifier: Option<&dyn BillingNotifier>,
) -> Result<i64, BillingError> {
    let new_balance =
        crate::ledger::deduct_credits(pool, account_id, amount, operation, description).await?;

    // Synchronous auto-topup with timeout to prevent blocking on DB issues
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        maybe_auto_topup(pool, account_id, payment_provider, notifier),
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
        if let Some(notifier) = notifier {
            if let Ok(uuid) = uuid::Uuid::parse_str(account_id) {
                notifier.notify_low_balance(pool, uuid, new_balance);
            }
        }
    }

    Ok(new_balance)
}

// ============================================================================
// Auto top-up
// ============================================================================

/// Check if the account's balance has dropped below its auto-topup threshold,
/// and if so, top up. When a payment provider is available and the account has a
/// saved payment method, it charges the card. Otherwise falls back to the free
/// (no-charge) top-up for dev/test.
pub async fn maybe_auto_topup(
    pool: &sqlx::PgPool,
    account_id: &str,
    payment_provider: Option<&dyn PaymentProvider>,
    notifier: Option<&dyn BillingNotifier>,
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

    // Try payment-provider-based auto-topup if available
    let has_stripe_pm: bool = row
        .get::<Option<String>, _>("stripe_default_payment_method_id")
        .is_some();

    if let Some(provider) = payment_provider {
        if has_stripe_pm {
            match provider
                .charge_auto_topup(pool, account_uuid, topup_amount)
                .await
            {
                Ok(_pi_id) => {
                    info!(
                        account_id,
                        topup_amount, "Auto top-up via payment provider completed"
                    );
                    if let Some(notifier) = notifier {
                        let amount_cents = calculate_price_cents(topup_amount);
                        let new_bal: i64 = sqlx::query_scalar(
                            "SELECT credits_balance FROM accounts WHERE id = $1",
                        )
                        .bind(account_uuid)
                        .fetch_optional(pool)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0);
                        notifier.notify_auto_topup_success(
                            pool,
                            account_uuid,
                            topup_amount,
                            amount_cents,
                            new_bal,
                        );
                    }
                    return;
                }
                Err(e) => {
                    warn!(account_id, error = %e, "Payment auto-topup failed, falling back to free topup");
                    if let Some(notifier) = notifier {
                        notifier.notify_auto_topup_failure(pool, account_uuid, &e);
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
