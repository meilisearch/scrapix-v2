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

pub use client::HyperlineClient;
pub use config::HyperlineConfig;
pub use error::HyperlineError;
