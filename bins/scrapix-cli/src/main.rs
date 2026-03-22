use clap::Parser;

use scrapix_cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = scrapix_cli::run(cli).await;
    std::process::exit(code);
}
