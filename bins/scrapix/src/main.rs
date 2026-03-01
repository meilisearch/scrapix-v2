//! Unified Scrapix binary
//!
//! Run individual services or all-in-one:
//!
//! ```bash
//! scrapix api          # Run the API server
//! scrapix frontier     # Run the frontier service
//! scrapix crawler      # Run the crawler worker
//! scrapix content      # Run the content worker
//! scrapix all          # Run everything in a single process
//! scrapix crawl ...    # CLI commands (same as before)
//! ```

mod all;

use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Scrapix — high-performance web crawler and search indexer
#[derive(Parser, Debug)]
#[command(name = "scrapix")]
#[command(version, about = "Scrapix web crawler — unified binary")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the API server
    Api(scrapix_api::Args),

    /// Run the frontier service
    Frontier(scrapix_frontier_service::Args),

    /// Run the crawler worker
    Crawler(scrapix_worker_crawler::Args),

    /// Run the content worker
    Content(scrapix_worker_content::Args),

    /// Run all services in a single process (API + Frontier + Crawler + Content)
    All(all::AllArgs),

    // --- CLI commands (forwarded to scrapix-cli) ---

    /// Start a new crawl job
    Crawl {
        /// Configuration file path (JSON)
        #[arg(short = 'p', long, group = "config_source")]
        config_path: Option<String>,

        /// Inline JSON configuration
        #[arg(short, long, group = "config_source")]
        config: Option<String>,

        /// Run synchronously (wait for completion)
        #[arg(long)]
        sync: bool,

        /// Follow events after job starts (async mode only)
        #[arg(short, long)]
        follow: bool,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Check job status
    Status {
        /// Job ID
        job_id: String,

        /// Watch mode
        #[arg(short, long)]
        watch: bool,

        /// Poll interval in seconds (for watch mode)
        #[arg(long, default_value = "2")]
        interval: u64,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Stream job events (SSE)
    Events {
        /// Job ID
        job_id: String,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// List recent jobs
    Jobs {
        /// Maximum number of jobs to list
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: usize,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Cancel a running job
    Cancel {
        /// Job ID
        job_id: String,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Check API server health
    Health {
        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Validate a configuration file
    Validate {
        /// Configuration file path (JSON)
        config_path: String,

        /// Output validation details
        #[arg(short, long)]
        verbose: bool,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Run a local crawl (without Kafka infrastructure)
    Local {
        /// Configuration file path (JSON)
        #[arg(short = 'p', long, group = "config_source")]
        config_path: Option<String>,

        /// Inline JSON configuration
        #[arg(short, long, group = "config_source")]
        config: Option<String>,

        /// Output file for results (JSON)
        #[arg(short, long)]
        output: Option<String>,

        /// Maximum concurrent requests
        #[arg(long, default_value = "10")]
        concurrency: usize,

        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,
    },

    /// Show system-wide statistics
    Stats {
        /// Include verbose details
        #[arg(short, long)]
        verbose: bool,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Show recent errors
    Errors {
        /// Number of recent errors to show
        #[arg(long, default_value = "20")]
        last: usize,

        /// Filter by job ID
        #[arg(long)]
        job: Option<String>,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Show per-domain statistics
    Domains {
        /// Number of top domains to show
        #[arg(long, default_value = "20")]
        top: usize,

        /// Filter by domain pattern
        #[arg(long)]
        filter: Option<String>,

        /// API server URL
        #[arg(short, long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
        api_url: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        output: scrapix_cli::OutputFormat,
    },

    /// Analytics commands (requires ClickHouse)
    #[command(subcommand)]
    Analytics(scrapix_cli::AnalyticsCommands),

    /// Run benchmarks
    #[command(subcommand)]
    Bench(scrapix_cli::BenchCommands),

    /// Kubernetes deployment management
    #[command(subcommand)]
    K8s(scrapix_cli::K8sCommands),

    /// Local infrastructure management (Docker Compose)
    #[command(subcommand)]
    Infra(scrapix_cli::InfraCommands),
}

fn init_tracing(verbose: bool) {
    let log_level = if verbose { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // --- Service commands ---
        Command::Api(args) => {
            init_tracing(args.verbose);
            scrapix_api::run(args).await
        }
        Command::Frontier(args) => {
            init_tracing(args.verbose);
            scrapix_frontier_service::run(args).await
        }
        Command::Crawler(args) => {
            init_tracing(args.verbose);
            scrapix_worker_crawler::run(args).await
        }
        Command::Content(args) => {
            init_tracing(args.verbose);
            scrapix_worker_content::run(args).await
        }
        Command::All(args) => {
            init_tracing(args.verbose);
            all::run_all(args).await
        }

        // --- CLI commands: convert to scrapix_cli::Cli and delegate ---
        Command::Crawl { config_path, config, sync, follow, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Crawl { config_path, config, sync, follow },
            }).await
        }
        Command::Status { job_id, watch, interval, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Status { job_id, watch, interval },
            }).await
        }
        Command::Events { job_id, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Events { job_id },
            }).await
        }
        Command::Jobs { limit, offset, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Jobs { limit, offset },
            }).await
        }
        Command::Cancel { job_id, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Cancel { job_id },
            }).await
        }
        Command::Health { api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Health,
            }).await
        }
        Command::Validate { config_path, verbose, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Validate { config_path, verbose },
            }).await
        }
        Command::Local { config_path, config, output, concurrency, verbose, api_url } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output: scrapix_cli::OutputFormat::Text,
                command: scrapix_cli::Commands::Local { config_path, config, output, concurrency, verbose },
            }).await
        }
        Command::Stats { verbose, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Stats { verbose },
            }).await
        }
        Command::Errors { last, job, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Errors { last, job },
            }).await
        }
        Command::Domains { top, filter, api_url, output } => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url, output,
                command: scrapix_cli::Commands::Domains { top, filter },
            }).await
        }
        Command::Analytics(cmd) => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url: std::env::var("SCRAPIX_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
                output: scrapix_cli::OutputFormat::Text,
                command: scrapix_cli::Commands::Analytics(cmd),
            }).await
        }
        Command::Bench(cmd) => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url: std::env::var("SCRAPIX_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
                output: scrapix_cli::OutputFormat::Text,
                command: scrapix_cli::Commands::Bench(cmd),
            }).await
        }
        Command::K8s(cmd) => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url: std::env::var("SCRAPIX_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
                output: scrapix_cli::OutputFormat::Text,
                command: scrapix_cli::Commands::K8s(cmd),
            }).await
        }
        Command::Infra(cmd) => {
            scrapix_cli::run(scrapix_cli::Cli {
                api_url: std::env::var("SCRAPIX_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
                output: scrapix_cli::OutputFormat::Text,
                command: scrapix_cli::Commands::Infra(cmd),
            }).await
        }
    }
}
