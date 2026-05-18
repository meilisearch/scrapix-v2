//! Idempotent seed runner: creates one `dynamic` product per
//! [`BillingEventType`] in the Hyperline workspace configured via
//! `HYPERLINE_API_KEY` + `HYPERLINE_API_BASE`.
//!
//! The actual seeding logic lives in
//! [`scrapix_billing_hyperline::products::seed_products`] so the API can
//! optionally run the same routine at boot when `HYPERLINE_AUTO_SEED=1`.
//! This binary stays useful for the one-shot ops workflow (or recovery
//! after a botched deploy where the auto-seed degraded).
//!
//! Each product is a `dynamic` billing primitive whose aggregator sums the
//! `credits` property across incoming events of the matching `event_type`.
//! Seeded with placeholder `amount: 0` pricing — ops still needs to set
//! real prices in the Hyperline UI afterward.
//!
//! Usage:
//!
//! ```sh
//! HYPERLINE_API_KEY=test_… cargo run -p scrapix-billing-hyperline \
//!     --example seed_hyperline_products
//! ```

use std::error::Error;
use std::process::ExitCode;

use scrapix_billing_hyperline::client::HyperlineClient;
use scrapix_billing_hyperline::products::seed_products;

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let client = HyperlineClient::from_env()?;
    let target = if client.config().is_sandbox() {
        "sandbox"
    } else {
        "PRODUCTION"
    };
    println!("Seeding Hyperline products into {target}…");

    match seed_products(&client).await {
        Ok(report) => {
            println!(
                "Done. created={} skipped={}",
                report.created, report.skipped
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("Seed failed: {e}");
            Ok(ExitCode::FAILURE)
        }
    }
}
