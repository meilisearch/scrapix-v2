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

/// Scan every account with a linked Hyperline wallet and return paired
/// balance observations. A per-account failure (wallet 404, network blip)
/// logs a warning and is skipped — one bad account never aborts the scan.
pub async fn scan_once(
    pool: &PgPool,
    client: &HyperlineClient,
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
                info!(
                    account_id = %drift.account_id,
                    local_credits = drift.local_credits,
                    hyperline_balance = drift.hyperline_balance,
                    hyperline_projected = ?drift.hyperline_projected_balance,
                    currency = ?drift.currency,
                    "reconcile: paired balance observation"
                );
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
pub fn spawn_scan_worker(
    pool: PgPool,
    client: HyperlineClient,
    interval: Duration,
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
            match scan_once(&pool, &client).await {
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
