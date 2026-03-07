use clap::Parser;

use scrapix_frontier_service::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    scrapix_core::telemetry::init_tracing(args.verbose);
    scrapix_frontier_service::run(args).await
}
