//! Scrapix Billing
//!
//! Credit ledger, pricing, auto-topup, and payment logic extracted from the
//! API server so it can be reused by workers, CLI tools, and future services.

pub mod auto_topup;
pub mod credits;
pub mod error;
pub mod ledger;
pub mod pricing;

// Re-export key types at the crate root for convenience.
pub use auto_topup::{BillingNotifier, PaymentProvider};
pub use credits::{crawl_credits_per_page, scrape_credits, MAP_CREDITS, SEARCH_CREDITS};
pub use error::BillingError;
pub use ledger::{add_credits_for_payment, check_credits, check_spend_limit, deduct_credits};
pub use pricing::calculate_price_cents;
