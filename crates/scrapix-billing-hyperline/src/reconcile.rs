//! Wallet-balance reconciliation between the local credit ledger and Hyperline.
//!
//! Two layers:
//!
//! - [`scan_once`] / [`spawn_scan_worker`] — for-every-linked-account pair
//!   the local `credits_balance` with the live Hyperline wallet balance and
//!   emit a paired observation. The worker never blocks or retries; it
//!   logs and moves on.
//!
//! - [`BalanceDrift`] — the paired observation shape. `is_within_tolerance`
//!   is deliberately **not** provided at the type level: local credits are
//!   our internal unit and Hyperline's `balance.amount` is the processor's
//!   minor currency unit (cents). Converting between the two is a
//!   pricing-policy decision that lives outside this module — callers that
//!   want drift alerts supply their own conversion when comparing.
//!
//! Per SCR-68 Phase 1: shadow-mode observability. No action is taken on
//! drift beyond logging — production alerting is a downstream concern that
//! consumes the scan output.
//!
//! Units are **not** normalized here on purpose. `local_credits` is in the
//! credit unit used by the ledger (`accounts.credits_balance`); both
//! `hyperline_balance` and `hyperline_projected_balance` are in the
//! smallest currency unit (cents for USD). The scan worker ships the raw
//! numbers; conversion is the consumer's job.

use std::time::Duration;

use serde::Serialize;
use sqlx::{PgPool, Row};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::client::HyperlineClient;
use crate::error::HyperlineError;

/// Paired observation: local credits vs. Hyperline wallet balance for a
/// single account. See module docs for the unit caveat.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceDrift {
    pub account_id: uuid::Uuid,
    /// Local `accounts.credits_balance` (credits, our internal unit).
    pub local_credits: i64,
    /// Hyperline `Wallet.balance.amount` — smallest currency unit.
    pub hyperline_balance: i64,
    /// Hyperline `Wallet.projected_balance.amount` — same unit as
    /// `hyperline_balance`. `None` when Hyperline didn't return it.
    pub hyperline_projected_balance: Option<i64>,
    /// ISO 4217 (e.g. `"USD"`) from the wallet, when present.
    pub currency: Option<String>,
}

/// Drift-alert configuration. Local credits and the Hyperline balance live
/// in different units (credits vs. cents), so the comparator needs a ratio
/// and an absolute tolerance to be meaningful.
///
/// The semantics: convert `local_credits` to cents via
/// `local_credits * cents_per_credit`, then alert when
/// `|local_cents - hyperline_balance| > tolerance_cents`.
#[derive(Debug, Clone, Copy)]
pub struct DriftThreshold {
    /// How many cents one local credit is worth. Site-specific pricing —
    /// no sensible default, must be set per deployment.
    pub cents_per_credit: i64,
    /// Tolerance in cents. Drift below this is normal noise (in-flight
    /// outbox rows, webhook lag) and stays at INFO; drift above gets a
    /// WARN line that ops dashboards alert on.
    pub tolerance_cents: i64,
}

impl DriftThreshold {
    /// Convert a paired observation into "drift in cents" using this
    /// threshold's ratio. Negative means the local ledger is *ahead* of
    /// Hyperline (we've debited more than they've billed); positive means
    /// behind (Hyperline has more credit than the ledger reflects).
    pub fn delta_cents(&self, drift: &BalanceDrift) -> i64 {
        drift
            .local_credits
            .saturating_mul(self.cents_per_credit)
            .saturating_sub(drift.hyperline_balance)
    }

    /// `true` when the absolute drift exceeds the tolerance.
    pub fn breached(&self, drift: &BalanceDrift) -> bool {
        self.delta_cents(drift).saturating_abs() > self.tolerance_cents
    }
}

/// Scan every account with a linked Hyperline wallet and return paired
/// balance observations. A per-account failure (wallet 404, network blip)
/// logs a warning and is skipped — one bad account never aborts the scan.
///
/// When `threshold` is `Some`, accounts whose drift exceeds the configured
/// tolerance get a `WARN` line per scan pass (alert-friendly). When `None`
/// the worker stays in pure observation mode — paired observations only,
/// no drift alerts.
pub async fn scan_once(
    pool: &PgPool,
    client: &HyperlineClient,
    threshold: Option<DriftThreshold>,
) -> Result<Vec<BalanceDrift>, HyperlineError> {
    let rows = sqlx::query(
        "SELECT id, credits_balance, hyperline_wallet_id \
         FROM accounts \
         WHERE hyperline_wallet_id IS NOT NULL \
           AND active = true",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::with_capacity(rows.len());
    for row in rows {
        let account_id: uuid::Uuid = row.try_get("id")?;
        let local_credits: i64 = row.try_get("credits_balance")?;
        let wallet_id: String = row.try_get("hyperline_wallet_id")?;

        match client.get_wallet(&wallet_id).await {
            Ok(wallet) => {
                let drift = BalanceDrift {
                    account_id,
                    local_credits,
                    hyperline_balance: wallet.balance.amount,
                    hyperline_projected_balance: wallet.projected_balance.map(|p| p.amount),
                    currency: wallet.currency,
                };
                if let Some(t) = threshold {
                    let delta = t.delta_cents(&drift);
                    if t.breached(&drift) {
                        warn!(
                            account_id = %drift.account_id,
                            local_credits = drift.local_credits,
                            hyperline_balance = drift.hyperline_balance,
                            cents_per_credit = t.cents_per_credit,
                            tolerance_cents = t.tolerance_cents,
                            delta_cents = delta,
                            currency = ?drift.currency,
                            "reconcile: balance drift exceeds tolerance"
                        );
                    } else {
                        info!(
                            account_id = %drift.account_id,
                            local_credits = drift.local_credits,
                            hyperline_balance = drift.hyperline_balance,
                            delta_cents = delta,
                            currency = ?drift.currency,
                            "reconcile: paired balance observation (within tolerance)"
                        );
                    }
                } else {
                    info!(
                        account_id = %drift.account_id,
                        local_credits = drift.local_credits,
                        hyperline_balance = drift.hyperline_balance,
                        hyperline_projected = ?drift.hyperline_projected_balance,
                        currency = ?drift.currency,
                        "reconcile: paired balance observation"
                    );
                }
                results.push(drift);
            }
            Err(e) => {
                warn!(
                    account_id = %account_id,
                    wallet_id = %wallet_id,
                    error = %e,
                    "reconcile: wallet fetch failed — skipping account"
                );
            }
        }
    }
    Ok(results)
}

/// Spawn the scan loop as a tokio task. Returns the `JoinHandle` so
/// callers can `abort()` on shutdown. A scan pass that errors at the DB
/// layer (not per-account) logs and the loop continues on the next tick.
///
/// `threshold` controls drift alerting — see [`scan_once`].
pub fn spawn_scan_worker(
    pool: PgPool,
    client: HyperlineClient,
    interval: Duration,
    threshold: Option<DriftThreshold>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Sensible first tick: give the app a moment after boot rather
        // than firing at t=0.
        let mut ticker = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(30),
            interval,
        );
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match scan_once(&pool, &client, threshold).await {
                Ok(drifts) => {
                    info!(count = drifts.len(), "reconcile: scan pass complete");
                }
                Err(e) => {
                    error!(error = %e, "reconcile: scan pass failed");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drift(local: i64, hyperline: i64) -> BalanceDrift {
        BalanceDrift {
            account_id: uuid::Uuid::nil(),
            local_credits: local,
            hyperline_balance: hyperline,
            hyperline_projected_balance: None,
            currency: Some("USD".to_string()),
        }
    }

    #[test]
    fn delta_cents_handles_ratio() {
        let t = DriftThreshold {
            cents_per_credit: 1,
            tolerance_cents: 1,
        };
        // 100 credits at 1¢/credit = 100¢; Hyperline says 100¢ → no drift.
        assert_eq!(t.delta_cents(&drift(100, 100)), 0);
        // 100 credits at 1¢/credit = 100¢; Hyperline says 90¢ → +10¢ ahead.
        assert_eq!(t.delta_cents(&drift(100, 90)), 10);
        // 100 credits at 1¢/credit = 100¢; Hyperline says 110¢ → -10¢ behind.
        assert_eq!(t.delta_cents(&drift(100, 110)), -10);
    }

    #[test]
    fn breached_respects_tolerance() {
        let t = DriftThreshold {
            cents_per_credit: 1,
            tolerance_cents: 5,
        };
        assert!(!t.breached(&drift(100, 100))); // 0 ≤ 5
        assert!(!t.breached(&drift(100, 95))); // 5 ≤ 5
        assert!(t.breached(&drift(100, 94))); // 6 > 5
        assert!(t.breached(&drift(100, 106))); // 6 > 5 (negative)
    }
}
