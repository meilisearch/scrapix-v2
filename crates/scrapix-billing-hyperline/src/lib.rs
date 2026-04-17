//! Scrapix — Hyperline billing provider.
//!
//! Replaces the direct Stripe integration in `bins/scrapix-api/src/stripe.rs`
//! by treating Hyperline as the billing system of record while Stripe acts as
//! a downstream payment processor configured inside Hyperline.
//!
//! Layout:
//! - [`client`]     — typed REST client over `HYPERLINE_API_BASE`.
//! - [`events`]     — usage event types + outbox-backed emission helper.
//! - [`outbox`]     — background drain worker that POSTs outbox rows to Hyperline.
//! - [`webhooks`]   — signature verification and payload parsing.
//! - [`reconcile`]  — wallet-balance drift checks between local ledger and Hyperline.
//! - [`config`]     — env-driven configuration.
//! - [`error`]      — crate-wide error type.

pub mod client;
pub mod config;
pub mod error;
pub mod events;
pub mod outbox;
pub mod reconcile;
pub mod webhooks;

pub use client::{Customer, HyperlineClient, MoneyAmount, PortalLink, Wallet};
pub use config::HyperlineConfig;
pub use error::HyperlineError;

/// Boot-time self-check: pings Hyperline to validate auth+network, and
/// logs the `BillingEventType` manifest that the API will emit.
///
/// Hyperline's product/pricing config lives in the Hyperline dashboard and
/// has no read-only introspection endpoint, so true bidirectional parity
/// isn't available. Instead we:
/// 1. Ping `/v1/customers?limit=1` — proves the API key is live and the
///    configured base URL is reachable.
/// 2. Emit an INFO log listing every `event_type` this process will POST
///    to `/v1/events`. Ops reads this off startup logs and cross-checks
///    that each event_type has a matching metered product in Hyperline.
///
/// On ping failure we log at WARN and return the error; the caller decides
/// whether to fail the boot (prod) or proceed in degraded local-only mode
/// (dev/CI). A transient outage shouldn't DoS the API — the outbox drain
/// worker already handles Hyperline downtime via retries — so production
/// deploys should log-and-continue rather than crashloop.
pub async fn boot_self_check(client: &HyperlineClient) -> Result<(), HyperlineError> {
    use events::BillingEventType;

    let manifest: Vec<&'static str> = BillingEventType::all().iter().map(|t| t.as_str()).collect();
    tracing::info!(
        sandbox = client.config().is_sandbox(),
        event_types = ?manifest,
        "Hyperline event-type manifest — verify each has a metered product in the Hyperline dashboard"
    );

    match client.ping().await {
        Ok(()) => {
            tracing::info!(
                sandbox = client.config().is_sandbox(),
                "Hyperline boot self-check passed"
            );
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                sandbox = client.config().is_sandbox(),
                "Hyperline boot self-check failed — API unreachable or auth invalid. Outbox will keep enqueuing and retry once Hyperline is reachable."
            );
            Err(e)
        }
    }
}
