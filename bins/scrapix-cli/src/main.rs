use clap::Parser;

use scrapix_cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    scrapix_cli::run(cli).await
}
