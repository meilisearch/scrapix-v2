use clap::Parser;

use scrapix_worker_content::Args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    scrapix_core::telemetry::init_tracing(args.verbose);
    scrapix_worker_content::run(args).await
}
