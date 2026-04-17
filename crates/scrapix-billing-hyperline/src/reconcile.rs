//! Wallet-balance reconciliation between the local credit ledger and Hyperline.
//!
//! Lands in Phase 1 of SCR-68 (shadow mode). For now this module only defines
//! the types consumed by the nightly drift-check job.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BalanceDrift {
    pub account_id: uuid::Uuid,
    pub local_balance_cents: i64,
    pub hyperline_balance_cents: i64,
}

impl BalanceDrift {
    pub fn is_within_tolerance(&self, tolerance_cents: i64) -> bool {
        (self.local_balance_cents - self.hyperline_balance_cents).abs() <= tolerance_cents
    }
}
