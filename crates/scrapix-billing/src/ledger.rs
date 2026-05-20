//! Credit ledger operations.
//!
//! Atomic credit checks, deductions, and transaction recording backed by
//! PostgreSQL. All balance mutations happen inside database transactions.

use sqlx::Row;
use tracing::{debug, info};

use crate::error::BillingError;

// ============================================================================
// Public API
// ============================================================================

/// Check that the account has at least `required_amount` credits.
/// Returns the current balance on success.
pub async fn check_credits(
    pool: &sqlx::PgPool,
    account_id: &str,
    required_amount: i64,
) -> Result<i64, BillingError> {
    let account_uuid = parse_uuid(account_id)?;

    let balance: i64 =
        sqlx::query_scalar("SELECT credits_balance FROM accounts WHERE id = $1 AND active = true")
            .bind(account_uuid)
            .fetch_optional(pool)
            .await?
            .ok_or(BillingError::AccountNotFound)?;

    if balance < required_amount {
        return Err(BillingError::InsufficientCredits {
            available: balance,
            required: required_amount,
        });
    }

    Ok(balance)
}

/// Atomically deduct credits from an account and record a transaction.
///
/// Uses `UPDATE ... WHERE credits_balance >= amount RETURNING credits_balance`
/// so the deduction only succeeds if the account has enough credits.
/// Returns the new balance on success.
pub async fn deduct_credits(
    pool: &sqlx::PgPool,
    account_id: &str,
    amount: i64,
    operation: &str,
    description: &str,
) -> Result<i64, BillingError> {
    let account_uuid = parse_uuid(account_id)?;

    let mut tx = pool.begin().await?;

    // Atomic check-and-deduct
    let new_balance: Option<i64> = sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance - $1 \
         WHERE id = $2 AND credits_balance >= $1 \
         RETURNING credits_balance",
    )
    .bind(amount)
    .bind(account_uuid)
    .fetch_optional(&mut *tx)
    .await?;

    let new_balance = match new_balance {
        Some(b) => b,
        None => {
            return Err(BillingError::InsufficientCredits {
                available: 0,
                required: amount,
            });
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
    .await?;

    tx.commit().await?;

    debug!(
        account_id = %account_id,
        amount,
        new_balance,
        operation,
        "Credits deducted"
    );

    Ok(new_balance)
}

/// Check if a top-up amount would exceed the monthly spend limit.
pub async fn check_spend_limit(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    amount: i64,
) -> Result<(), BillingError> {
    let row = sqlx::query("SELECT monthly_spend_limit FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await?
        .ok_or(BillingError::AccountNotFound)?;

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
        .await?;

        if spent + amount > limit {
            return Err(BillingError::SpendLimitExceeded);
        }
    }

    Ok(())
}

/// Add credits to an account after a successful payment.
/// Idempotent: checks if a transaction with this `stripe_payment_intent_id` already exists.
pub async fn add_credits_for_payment(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    credits: i64,
    payment_intent_id: &str,
    description: &str,
) -> Result<(), BillingError> {
    // Idempotency check
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM transactions WHERE account_id = $1 AND metadata->>'stripe_payment_intent_id' = $2)",
    )
    .bind(account_id)
    .bind(payment_intent_id)
    .fetch_one(pool)
    .await?;

    if exists {
        info!(account_id = %account_id, pi = %payment_intent_id, "Payment already processed, skipping");
        return Ok(());
    }

    let mut tx = pool.begin().await?;

    let new_balance: i64 = sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance + $1 WHERE id = $2 RETURNING credits_balance",
    )
    .bind(credits)
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await?;

    let metadata = serde_json::json!({
        "stripe_payment_intent_id": payment_intent_id,
    });

    sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description, metadata) \
         VALUES ($1, 'manual_topup', $2, $3, $4, $5)",
    )
    .bind(account_id)
    .bind(credits)
    .bind(new_balance)
    .bind(description)
    .bind(metadata)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    info!(
        account_id = %account_id,
        credits,
        new_balance,
        pi = %payment_intent_id,
        "Credits added via payment"
    );

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

pub(crate) fn parse_uuid(id: &str) -> Result<uuid::Uuid, BillingError> {
    uuid::Uuid::parse_str(id).map_err(|_| BillingError::InvalidAccountId(id.to_string()))
}
