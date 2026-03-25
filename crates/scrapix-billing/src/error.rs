//! Billing-specific error types.
//!
//! These are independent of any HTTP framework so the crate can be used
//! from workers, CLI tools, or the API server.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BillingError {
    #[error("Insufficient credits: {available} available, {required} required")]
    InsufficientCredits { available: i64, required: i64 },

    #[error("Account not found or inactive")]
    AccountNotFound,

    #[error("Monthly spend limit reached")]
    SpendLimitExceeded,

    #[error("Invalid account ID: {0}")]
    InvalidAccountId(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Payment error: {0}")]
    Payment(String),
}

impl BillingError {
    /// Returns an error code string suitable for API responses.
    pub fn code(&self) -> &'static str {
        match self {
            BillingError::InsufficientCredits { .. } => "insufficient_credits",
            BillingError::AccountNotFound => "not_found",
            BillingError::SpendLimitExceeded => "spend_limit_exceeded",
            BillingError::InvalidAccountId(_) => "internal_error",
            BillingError::Database(_) => "internal_error",
            BillingError::Payment(_) => "payment_error",
        }
    }
}
