//! Shared telemetry initialization for all Scrapix services

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize tracing with the given verbosity level.
///
/// Uses `RUST_LOG` environment variable if set, otherwise falls back to
/// "debug" (if `verbose` is true) or "info".
pub fn init_tracing(verbose: bool) {
    let log_level = if verbose { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();
}
