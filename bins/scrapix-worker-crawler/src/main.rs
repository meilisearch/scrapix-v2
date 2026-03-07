use clap::Parser;

use scrapix_worker_crawler::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    scrapix_core::telemetry::init_tracing(args.verbose);
    scrapix_worker_crawler::run(args).await
}
