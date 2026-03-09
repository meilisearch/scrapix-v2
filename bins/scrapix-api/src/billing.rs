//! Credit consumption and billing logic.
//!
//! All credit operations are centralized here: balance checks, atomic
//! deductions, auto-topup, and spend-limit enforcement.

use sqlx::Row;
use tracing::{debug, error, info, warn};

use crate::ApiError;

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

    let balance: i64 = sqlx::query_scalar("SELECT credits_balance FROM accounts WHERE id = $1")
        .bind(account_uuid)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to query credits balance");
            ApiError::new("Database error", "internal_error")
        })?
        .ok_or_else(|| ApiError::new("Account not found", "not_found"))?;

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

/// Combined atomic check-and-deduct, with auto-topup spawned as a background
/// task if the balance drops below the threshold.
///
/// Returns the new balance on success.
pub(crate) async fn check_credits_and_deduct(
    pool: &sqlx::PgPool,
    account_id: &str,
    amount: i64,
    operation: &str,
    description: &str,
) -> Result<i64, ApiError> {
    let new_balance = deduct_credits(pool, account_id, amount, operation, description).await?;

    // Fire-and-forget auto-topup check
    let pool = pool.clone();
    let account_id = account_id.to_string();
    tokio::spawn(async move {
        maybe_auto_topup(&pool, &account_id).await;
    });

    Ok(new_balance)
}

/// Check if the account's balance has dropped below its auto-topup threshold,
/// and if so, top up. This is fire-and-forget; errors are logged but not
/// propagated.
pub(crate) async fn maybe_auto_topup(pool: &sqlx::PgPool, account_id: &str) {
    let account_uuid = match uuid::Uuid::parse_str(account_id) {
        Ok(u) => u,
        Err(_) => return,
    };

    // Read account settings
    let row = match sqlx::query(
        "SELECT credits_balance, auto_topup_enabled, auto_topup_amount, auto_topup_threshold \
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

    // Perform the top-up
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
        topup_amount, new_balance, "Auto top-up completed"
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
            "SELECT COALESCE(SUM(amount), 0) FROM transactions \
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
                format!(
                    "Monthly spend limit reached ({} of {} used this month)",
                    spent, limit
                ),
                "spend_limit_exceeded",
            ));
        }
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_uuid(id: &str) -> Result<uuid::Uuid, ApiError> {
    uuid::Uuid::parse_str(id).map_err(|_| ApiError::new("Invalid account ID", "internal_error"))
}
