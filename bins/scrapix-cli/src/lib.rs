pub mod client;
pub mod commands;
pub mod config;
pub mod output;
pub mod types;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

use client::ApiClient;
use config::CliConfig;
use output::print_error;

// ============================================================================
// Exit codes
// ============================================================================

const EXIT_SUCCESS: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_AUTH_ERROR: i32 = 2;
const EXIT_VALIDATION_ERROR: i32 = 3;

// ============================================================================
// CLI Definition
// ============================================================================

/// Scrapix web crawler CLI
#[derive(Parser, Debug)]
#[command(name = "scrapix")]
#[command(about = "Scrapix — web crawling & search indexing for humans and AI agents")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    /// API server URL
    #[arg(long, env = "SCRAPIX_API_URL", global = true)]
    pub api_url: Option<String>,

    /// API key for authentication
    #[arg(long, env = "SCRAPIX_API_KEY", global = true)]
    pub api_key: Option<String>,

    /// Output as JSON (default: human-friendly text)
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    // ── Authentication ──────────────────────────────────
    /// Authenticate via browser (OAuth) or API key
    Login {
        /// Use API key instead of browser login
        #[arg(long)]
        api_key: bool,
    },

    /// Clear stored credentials
    Logout,

    /// Show current user and account
    Whoami,

    /// Show auth status and credits remaining
    Status,

    // ── Core API ────────────────────────────────────────
    /// Scrape a single URL
    Scrape {
        /// URL to scrape
        url: String,

        /// Output format (md, html, text, raw, links, metadata)
        #[arg(short, long)]
        format: Option<String>,

        /// Only return main content
        #[arg(long)]
        main_content: bool,

        /// Enable JavaScript rendering
        #[arg(long)]
        js: bool,

        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,

        /// CSS selector extraction (repeatable)
        #[arg(long)]
        extract: Vec<String>,

        /// Add AI summary
        #[arg(long)]
        ai_summary: bool,

        /// AI-powered extraction with prompt
        #[arg(long)]
        ai_extract: Option<String>,
    },

    /// Discover all URLs on a site
    Map {
        /// URL to map
        url: String,

        /// Max links to return (default: 5000)
        #[arg(long)]
        limit: Option<u32>,

        /// BFS depth (default: 0)
        #[arg(long)]
        depth: Option<u32>,

        /// Filter by term
        #[arg(long)]
        search: Option<String>,

        /// Skip sitemap discovery
        #[arg(long)]
        no_sitemap: bool,

        /// Skip title/description fetching
        #[arg(long)]
        no_metadata: bool,
    },

    /// Search indexed content
    Search {
        /// Base URL or index
        url: String,

        /// Search query
        #[arg(short, long)]
        q: String,

        /// Results per page
        #[arg(long)]
        limit: Option<u32>,

        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,

        /// Meilisearch filter expression
        #[arg(long)]
        filter: Option<String>,

        /// Sort rule (repeatable)
        #[arg(long)]
        sort: Vec<String>,
    },

    /// Start an async crawl job
    Crawl {
        /// Configuration file path (JSON)
        config: Option<String>,

        /// Run synchronously (wait for completion)
        #[arg(long)]
        sync: bool,

        /// Stream events after start
        #[arg(short, long)]
        follow: bool,

        /// Inline config JSON instead of file
        #[arg(long)]
        inline: Option<String>,
    },

    // ── Job Management ──────────────────────────────────
    /// List recent jobs
    Jobs {
        /// Max jobs to list
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Pagination offset
        #[arg(long, default_value = "0")]
        offset: usize,

        /// Filter by status (running, completed, failed)
        #[arg(long)]
        status: Option<String>,
    },

    /// Show job status or manage a job
    Job {
        /// Job ID
        id: String,

        /// Poll continuously
        #[arg(short, long)]
        watch: bool,

        /// Stream SSE events
        #[arg(short, long)]
        events: bool,

        #[command(subcommand)]
        action: Option<JobAction>,
    },

    // ── Configs ─────────────────────────────────────────
    /// List or manage saved crawl configs
    Configs {
        /// Max configs
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Pagination offset
        #[arg(long, default_value = "0")]
        offset: usize,
    },

    /// Show, create, update, delete, or trigger a config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    // ── Engines ─────────────────────────────────────────
    /// List Meilisearch engines
    Engines,

    /// Manage a Meilisearch engine
    Engine {
        #[command(subcommand)]
        action: EngineAction,
    },

    // ── API Keys ────────────────────────────────────────
    /// List API keys
    ApiKeys,

    /// Create or revoke an API key
    ApiKey {
        #[command(subcommand)]
        action: ApiKeyAction,
    },

    // ── Billing ─────────────────────────────────────────
    /// Show billing info and credits
    Billing {
        #[command(subcommand)]
        action: Option<BillingAction>,
    },

    // ── Team ────────────────────────────────────────────
    /// Manage team members
    Team {
        #[command(subcommand)]
        action: Option<TeamAction>,
    },

    // ── Diagnostics ─────────────────────────────────────
    /// System statistics
    Stats,

    /// Recent errors
    Errors {
        /// Number of errors to show
        #[arg(long, default_value = "20")]
        last: usize,

        /// Filter by job ID
        #[arg(long)]
        job: Option<String>,
    },

    /// Per-domain statistics
    Domains {
        /// Number of top domains
        #[arg(long, default_value = "20")]
        top: usize,

        /// Filter by domain pattern
        #[arg(long)]
        filter: Option<String>,
    },

    /// API health check
    Health,

    // ── Analytics ───────────────────────────────────────
    /// ClickHouse analytics
    Analytics {
        #[command(subcommand)]
        action: AnalyticsAction,
    },

    // ── Infrastructure ──────────────────────────────────
    /// Local infrastructure management (Docker Compose)
    Infra {
        #[command(subcommand)]
        action: InfraAction,
    },

    /// Kubernetes deployment management
    K8s {
        #[command(subcommand)]
        action: K8sAction,
    },

    // ── Local ───────────────────────────────────────────
    /// Run a standalone crawl without infrastructure
    Local {
        /// Configuration file path (JSON)
        config: Option<String>,

        /// Output file for results
        #[arg(short, long)]
        output: Option<String>,

        /// Max concurrent requests
        #[arg(long, default_value = "10")]
        concurrency: usize,

        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,

        /// Inline config JSON
        #[arg(long)]
        inline: Option<String>,
    },

    // ── Utility ─────────────────────────────────────────
    /// Validate a configuration file
    Validate {
        /// Configuration file path
        config_path: String,

        /// Show details
        #[arg(short, long)]
        verbose: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell type (bash, zsh, fish, powershell, elvish)
        shell: Shell,
    },

    /// Run benchmarks
    Bench {
        #[command(subcommand)]
        action: BenchAction,
    },
}

// ── Nested subcommands ──────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum JobAction {
    /// Cancel a running job
    Cancel,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show config details
    Show { id: String },
    /// Create config from JSON file
    Create { file: String },
    /// Update config
    Update { id: String, file: String },
    /// Delete config
    Delete { id: String },
    /// Trigger crawl from config
    Trigger { id: String },
}

#[derive(Subcommand, Debug)]
pub enum EngineAction {
    /// Show engine details
    Show { id: String },
    /// Create a new engine
    Create {
        /// Engine name
        #[arg(long)]
        name: String,
        /// Meilisearch URL
        #[arg(long)]
        url: String,
        /// Meilisearch API key
        #[arg(long)]
        api_key: Option<String>,
        /// Set as default
        #[arg(long)]
        default: bool,
    },
    /// Update engine
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Delete engine
    Delete { id: String },
    /// Set as default engine
    Default { id: String },
    /// List indexes on engine
    Indexes { id: String },
}

#[derive(Subcommand, Debug)]
pub enum ApiKeyAction {
    /// Create a new API key
    Create {
        /// Key name
        #[arg(long)]
        name: String,
    },
    /// Revoke an API key
    Revoke { id: String },
}

#[derive(Subcommand, Debug)]
pub enum BillingAction {
    /// List billing transactions
    Transactions {
        #[arg(short, long, default_value = "20")]
        limit: usize,
        #[arg(long, default_value = "0")]
        offset: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum TeamAction {
    /// Invite a new team member
    Invite {
        email: String,
        #[arg(long)]
        role: Option<String>,
    },
    /// Remove a team member
    Remove { user_id: String },
    /// Change a member's role
    Role { user_id: String, role: String },
}

#[derive(Subcommand, Debug)]
pub enum AnalyticsAction {
    /// List available pipes
    Pipes,
    /// Key performance indicators
    Kpis {
        #[arg(long, default_value = "24")]
        hours: u32,
    },
    /// Top domains by request count
    TopDomains {
        #[arg(long, default_value = "24")]
        hours: u32,
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Statistics for a specific domain
    Domain {
        domain: String,
        #[arg(long, default_value = "24")]
        hours: u32,
    },
    /// Hourly crawl statistics
    Hourly {
        #[arg(long, default_value = "24")]
        hours: u32,
    },
    /// Error distribution by status code
    Errors {
        #[arg(long, default_value = "24")]
        hours: u32,
    },
    /// Statistics for a specific job
    Job { id: String },
}

#[derive(Subcommand, Debug)]
pub enum InfraAction {
    /// Start infrastructure services
    Up,
    /// Stop infrastructure services
    Down,
    /// Restart infrastructure services
    Restart,
    /// Show status
    Status,
    /// View logs
    Logs {
        service: Option<String>,
        #[arg(short, long)]
        follow: bool,
    },
    /// Stop and remove all data volumes
    Reset {
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum K8sAction {
    /// Deploy to Kubernetes
    Deploy {
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
        #[arg(short, long, default_value = "local")]
        overlay: String,
    },
    /// Remove deployment
    Destroy {
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
        #[arg(short, long, default_value = "local")]
        overlay: String,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Show deployment status
    Status {
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
        #[arg(short, long)]
        watch: bool,
    },
    /// Stream component logs
    Logs {
        #[arg(default_value = "all")]
        component: String,
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
        #[arg(short, long)]
        follow: bool,
    },
    /// Scale a component
    Scale {
        component: String,
        #[arg(short, long)]
        replicas: u32,
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
    },
    /// Restart a component
    Restart {
        #[arg(default_value = "all")]
        component: String,
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
    },
    /// Forward ports for local access
    PortForward {
        #[arg(short, long, default_value = "scrapix")]
        namespace: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum BenchAction {
    /// Run all benchmarks
    All {
        #[arg(short, long, default_value = "./bench-results")]
        output: String,
        #[arg(short, long, default_value = "1")]
        iterations: u32,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Run Wikipedia E2E benchmark
    Wikipedia {
        #[arg(short, long, default_value = "./bench-results")]
        output: String,
        #[arg(short, long, default_value = "1")]
        iterations: u32,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Run integrated benchmarks
    Integrated {
        #[arg(short, long, default_value = "./bench-results")]
        output: String,
        #[arg(short, long, default_value = "1")]
        iterations: u32,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Run parser benchmarks
    Parser {
        #[arg(short, long, default_value = "./bench-results")]
        output: String,
        #[arg(short, long)]
        verbose: bool,
    },
}

// ============================================================================
// Run dispatcher
// ============================================================================

pub async fn run(cli: Cli) -> i32 {
    match run_inner(cli).await {
        Ok(()) => EXIT_SUCCESS,
        Err(e) => {
            let msg = format!("{:#}", e);
            print_error(&msg);

            // Determine exit code from error
            if msg.contains("auth")
                || msg.contains("unauthorized")
                || msg.contains("Unauthorized")
                || msg.contains("API key")
            {
                EXIT_AUTH_ERROR
            } else if msg.contains("validation") || msg.contains("invalid") {
                EXIT_VALIDATION_ERROR
            } else {
                EXIT_ERROR
            }
        }
    }
}

async fn run_inner(cli: Cli) -> Result<()> {
    let json = cli.json;

    // Resolve API URL and key: flags > env > config
    let mut cfg = CliConfig::load().unwrap_or_default();
    let api_url = cli
        .api_url
        .or(cfg.api_url.clone())
        .unwrap_or_else(|| "http://localhost:8080".to_string());

    // Also check if config says json
    let json = json || cfg.output.as_deref() == Some("json");

    // Resolve auth: CLI flag > env > config (with token refresh)
    let auth = if let Some(ref key) = cli.api_key {
        Some(config::AuthCredential::ApiKey(key.clone()))
    } else {
        // Try auto-refresh if we have an expired OAuth token
        if cfg.access_token.is_some() && !cfg.has_valid_token() {
            let _ = commands::auth::refresh_token_if_needed(&api_url, &mut cfg).await;
        }
        cfg.auth_credential()
    };

    let client = ApiClient::new(&api_url, auth);

    match cli.command {
        // ── Authentication ──────────────────────────────
        Commands::Login { api_key } => commands::auth::handle_login(&api_url, api_key).await,
        Commands::Logout => commands::auth::handle_logout().await,
        Commands::Whoami => commands::auth::handle_whoami(&client, json).await,
        Commands::Status => commands::auth::handle_status_auth(&client, json).await,

        // ── Core API ────────────────────────────────────
        Commands::Scrape {
            url,
            format,
            main_content,
            js,
            timeout,
            extract,
            ai_summary,
            ai_extract,
        } => {
            commands::scrape::handle_scrape(
                &client,
                url,
                format,
                main_content,
                js,
                timeout,
                extract,
                ai_summary,
                ai_extract,
                json,
            )
            .await
        }

        Commands::Map {
            url,
            limit,
            depth,
            search,
            no_sitemap,
            no_metadata,
        } => {
            commands::map::handle_map(
                &client,
                url,
                limit,
                depth,
                search,
                no_sitemap,
                no_metadata,
                json,
            )
            .await
        }

        Commands::Search {
            url,
            q,
            limit,
            offset,
            filter,
            sort,
        } => {
            commands::search::handle_search(&client, url, q, limit, offset, filter, sort, json)
                .await
        }

        Commands::Crawl {
            config,
            sync,
            follow,
            inline,
        } => commands::crawl::handle_crawl(&client, config, inline, sync, follow, json).await,

        // ── Job Management ──────────────────────────────
        Commands::Jobs {
            limit,
            offset,
            status,
        } => commands::crawl::handle_jobs(&client, limit, offset, status, json).await,

        Commands::Job {
            id,
            watch,
            events,
            action,
        } => match action {
            Some(JobAction::Cancel) => commands::crawl::handle_job_cancel(&client, &id, json).await,
            None => commands::crawl::handle_job(&client, &id, watch, events, json).await,
        },

        // ── Configs ─────────────────────────────────────
        Commands::Configs { limit, offset } => {
            commands::configs::handle_configs_list(&client, limit, offset, json).await
        }

        Commands::Config { action } => match action {
            ConfigAction::Show { id } => {
                commands::configs::handle_config_show(&client, &id, json).await
            }
            ConfigAction::Create { file } => {
                commands::configs::handle_config_create(&client, &file, json).await
            }
            ConfigAction::Update { id, file } => {
                commands::configs::handle_config_update(&client, &id, &file, json).await
            }
            ConfigAction::Delete { id } => {
                commands::configs::handle_config_delete(&client, &id, json).await
            }
            ConfigAction::Trigger { id } => {
                commands::configs::handle_config_trigger(&client, &id, json).await
            }
        },

        // ── Engines ─────────────────────────────────────
        Commands::Engines => commands::engines::handle_engines_list(&client, json).await,

        Commands::Engine { action } => match action {
            EngineAction::Show { id } => {
                commands::engines::handle_engine_show(&client, &id, json).await
            }
            EngineAction::Create {
                name,
                url,
                api_key,
                default,
            } => {
                commands::engines::handle_engine_create(&client, name, url, api_key, default, json)
                    .await
            }
            EngineAction::Update {
                id,
                name,
                url,
                api_key,
            } => {
                commands::engines::handle_engine_update(&client, &id, name, url, api_key, json)
                    .await
            }
            EngineAction::Delete { id } => {
                commands::engines::handle_engine_delete(&client, &id, json).await
            }
            EngineAction::Default { id } => {
                commands::engines::handle_engine_default(&client, &id, json).await
            }
            EngineAction::Indexes { id } => {
                commands::engines::handle_engine_indexes(&client, &id, json).await
            }
        },

        // ── API Keys ────────────────────────────────────
        Commands::ApiKeys => commands::api_keys::handle_api_keys_list(&client, json).await,

        Commands::ApiKey { action } => match action {
            ApiKeyAction::Create { name } => {
                commands::api_keys::handle_api_key_create(&client, name, json).await
            }
            ApiKeyAction::Revoke { id } => {
                commands::api_keys::handle_api_key_revoke(&client, &id, json).await
            }
        },

        // ── Billing ─────────────────────────────────────
        Commands::Billing { action } => match action {
            None => commands::billing::handle_billing(&client, json).await,
            Some(BillingAction::Transactions { limit, offset }) => {
                commands::billing::handle_billing_transactions(&client, limit, offset, json).await
            }
        },

        // ── Team ────────────────────────────────────────
        Commands::Team { action } => match action {
            None => commands::team::handle_team_list(&client, json).await,
            Some(TeamAction::Invite { email, role }) => {
                commands::team::handle_team_invite(&client, email, role, json).await
            }
            Some(TeamAction::Remove { user_id }) => {
                commands::team::handle_team_remove(&client, &user_id, json).await
            }
            Some(TeamAction::Role { user_id, role }) => {
                commands::team::handle_team_role(&client, &user_id, role, json).await
            }
        },

        // ── Diagnostics ─────────────────────────────────
        Commands::Health => commands::diagnostics::handle_health(&client, json).await,
        Commands::Stats => commands::diagnostics::handle_stats(&client, json).await,
        Commands::Errors { last, job } => {
            commands::diagnostics::handle_errors(&client, last, job, json).await
        }
        Commands::Domains { top, filter } => {
            commands::diagnostics::handle_domains(&client, top, filter, json).await
        }

        // ── Analytics ───────────────────────────────────
        Commands::Analytics { action } => match action {
            AnalyticsAction::Pipes => {
                commands::analytics::handle_analytics_pipes(&client, json).await
            }
            AnalyticsAction::Kpis { hours } => {
                commands::analytics::handle_analytics_kpis(&client, hours, json).await
            }
            AnalyticsAction::TopDomains { hours, limit } => {
                commands::analytics::handle_analytics_top_domains(&client, hours, limit, json).await
            }
            AnalyticsAction::Domain { domain, hours } => {
                commands::analytics::handle_analytics_domain(&client, &domain, hours, json).await
            }
            AnalyticsAction::Hourly { hours } => {
                commands::analytics::handle_analytics_hourly(&client, hours, json).await
            }
            AnalyticsAction::Errors { hours } => {
                commands::analytics::handle_analytics_errors(&client, hours, json).await
            }
            AnalyticsAction::Job { id } => {
                commands::analytics::handle_analytics_job(&client, &id, json).await
            }
        },

        // ── Infrastructure ──────────────────────────────
        Commands::Infra { action } => match action {
            InfraAction::Up => commands::infra::handle_infra_up(json),
            InfraAction::Down => commands::infra::handle_infra_down(json),
            InfraAction::Restart => commands::infra::handle_infra_restart(json),
            InfraAction::Status => commands::infra::handle_infra_status(),
            InfraAction::Logs { service, follow } => {
                commands::infra::handle_infra_logs(service, follow)
            }
            InfraAction::Reset { yes } => commands::infra::handle_infra_reset(yes, json),
        },

        // ── Kubernetes ──────────────────────────────────
        Commands::K8s { action } => match action {
            K8sAction::Deploy { namespace, overlay } => {
                commands::k8s::handle_k8s_deploy(&namespace, &overlay, json)
            }
            K8sAction::Destroy {
                namespace,
                overlay,
                yes,
            } => commands::k8s::handle_k8s_destroy(&namespace, &overlay, yes, json),
            K8sAction::Status { namespace, watch } => {
                commands::k8s::handle_k8s_status(&namespace, watch, json)
            }
            K8sAction::Logs {
                component,
                namespace,
                follow,
            } => commands::k8s::handle_k8s_logs(&component, &namespace, follow),
            K8sAction::Scale {
                component,
                replicas,
                namespace,
            } => commands::k8s::handle_k8s_scale(&component, replicas, &namespace, json),
            K8sAction::Restart {
                component,
                namespace,
            } => commands::k8s::handle_k8s_restart(&component, &namespace, json),
            K8sAction::PortForward { namespace } => {
                commands::k8s::handle_k8s_port_forward(&namespace, json)
            }
        },

        // ── Local ───────────────────────────────────────
        Commands::Local {
            config,
            output,
            concurrency,
            verbose,
            inline,
        } => {
            commands::local::handle_local(config, inline, output, concurrency, verbose, json).await
        }

        // ── Utility ─────────────────────────────────────
        Commands::Validate {
            config_path,
            verbose,
        } => handle_validate(&config_path, verbose, json).await,

        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "scrapix", &mut std::io::stdout());
            Ok(())
        }

        Commands::Bench { action } => handle_bench(action, json),
    }
}

// ============================================================================
// Validate (kept here since it's small and uses scrapix_core directly)
// ============================================================================

async fn handle_validate(config_path: &str, verbose: bool, json: bool) -> Result<()> {
    use scrapix_core::CrawlConfig;

    let content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path))?;

    let config: CrawlConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", config_path))?;

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if config.start_urls.is_empty() {
        errors.push("start_urls is empty - at least one URL is required".to_string());
    }
    if config.index_uid.is_empty() {
        errors.push("index_uid is empty - required for indexing".to_string());
    }

    for (i, url) in config.start_urls.iter().enumerate() {
        if url::Url::parse(url).is_err() {
            errors.push(format!("start_urls[{}]: invalid URL '{}'", i, url));
        }
    }

    if config.max_depth.is_none() && config.max_pages.is_none() {
        warnings.push("No max_depth or max_pages set - crawl may run indefinitely".to_string());
    }

    if let Some(depth) = config.max_depth {
        if depth > 10 {
            warnings.push(format!(
                "max_depth={} is quite deep - consider a smaller value",
                depth
            ));
        }
    }

    if json {
        let result = serde_json::json!({
            "valid": errors.is_empty(),
            "errors": errors,
            "warnings": warnings,
            "config": if verbose { Some(&config) } else { None }
        });
        output::print_json(&result);
    } else {
        use colored::Colorize;

        if errors.is_empty() {
            output::print_success(&format!("Configuration '{}' is valid", config_path));
        } else {
            output::print_error(&format!(
                "Configuration '{}' has {} error(s)",
                config_path,
                errors.len()
            ));
        }

        if !errors.is_empty() {
            eprintln!("{}", "Errors:".bold().red());
            for error in &errors {
                eprintln!("  {} {}", "✗".red(), error);
            }
        }

        if !warnings.is_empty() {
            eprintln!("{}", "Warnings:".bold().yellow());
            for warning in &warnings {
                eprintln!("  {} {}", "⚠".yellow(), warning);
            }
        }

        if verbose {
            eprintln!("{}", "Configuration Details:".bold());
            eprintln!("  {} {}", "Index UID:".dimmed(), config.index_uid);
            eprintln!("  {} {}", "Start URLs:".dimmed(), config.start_urls.len());
            eprintln!("  {} {:?}", "Crawler Type:".dimmed(), config.crawler_type);
            if let Some(depth) = config.max_depth {
                eprintln!("  {} {}", "Max Depth:".dimmed(), depth);
            }
            if let Some(pages) = config.max_pages {
                eprintln!("  {} {}", "Max Pages:".dimmed(), pages);
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Configuration validation failed")
    }
}

// ============================================================================
// Bench (kept here since it uses std::process::Command)
// ============================================================================

fn handle_bench(action: BenchAction, json: bool) -> Result<()> {
    let (benches, output_dir, iterations, verbose) = match action {
        BenchAction::All {
            output,
            iterations,
            verbose,
        } => (
            vec!["wikipedia_e2e", "integrated_benchmarks"],
            output,
            iterations,
            verbose,
        ),
        BenchAction::Wikipedia {
            output,
            iterations,
            verbose,
        } => (vec!["wikipedia_e2e"], output, iterations, verbose),
        BenchAction::Integrated {
            output,
            iterations,
            verbose,
        } => (vec!["integrated_benchmarks"], output, iterations, verbose),
        BenchAction::Parser { output, verbose } => {
            (vec!["integrated_benchmarks"], output, 1, verbose)
        }
    };

    if !json {
        output::print_info(&format!("Benchmarks: {}", benches.join(", ")));
        output::print_info(&format!("Output: {}", output_dir));
    }

    std::fs::create_dir_all(&output_dir)?;

    if !json {
        output::print_info("Ensuring release build...");
    }

    let build_status = std::process::Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .context("Failed to run cargo build")?;

    if !build_status.success() {
        anyhow::bail!("Build failed");
    }

    let total_start = std::time::Instant::now();
    let mut results: Vec<serde_json::Value> = Vec::new();

    for iteration in 1..=iterations {
        if iterations > 1 && !json {
            output::print_info(&format!(
                "=== Iteration {} of {} ===",
                iteration, iterations
            ));
        }

        for bench in &benches {
            let start = std::time::Instant::now();

            if !json {
                output::print_info(&format!("Running: {}", bench));
            }

            let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
            let output_file = format!("{}/{}-{}.txt", output_dir, bench, timestamp);

            let cmd_output = std::process::Command::new("cargo")
                .args(["bench", "--bench", bench])
                .output()
                .context("Failed to run cargo bench")?;

            let duration = start.elapsed();
            std::fs::write(&output_file, &cmd_output.stdout)?;

            if !json {
                if verbose {
                    eprintln!("{}", String::from_utf8_lossy(&cmd_output.stdout));
                }

                output::print_success(&format!(
                    "'{}' completed in {:.2}s",
                    bench,
                    duration.as_secs_f64()
                ));

                let stdout = String::from_utf8_lossy(&cmd_output.stdout);
                for line in stdout.lines() {
                    if line.contains("time:") || line.contains("thrpt:") {
                        eprintln!("  {}", line);
                    }
                }
            }

            results.push(serde_json::json!({
                "benchmark": bench,
                "iteration": iteration,
                "duration_seconds": duration.as_secs_f64(),
                "output_file": output_file,
                "success": cmd_output.status.success(),
            }));
        }
    }

    let total_duration = total_start.elapsed();

    if json {
        output::print_json(&serde_json::json!({
            "benchmarks": benches,
            "iterations": iterations,
            "total_duration_seconds": total_duration.as_secs_f64(),
            "results": results,
        }));
    } else {
        output::print_success(&format!(
            "All benchmarks completed in {:.2}s",
            total_duration.as_secs_f64()
        ));
    }

    Ok(())
}
