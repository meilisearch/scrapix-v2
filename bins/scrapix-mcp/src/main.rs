//! Scrapix MCP Server
//!
//! Exposes all Scrapix API endpoints as MCP tools, allowing AI agents
//! (Claude, etc.) to interact with Scrapix programmatically.
//!
//! The server reads the OpenAPI spec from a running Scrapix API instance
//! and generates MCP tools automatically via `rmcp-openapi`.
//!
//! ## Usage
//!
//! ```bash
//! # Stdio transport (default — for Claude Code, Cursor, etc.)
//! scrapix-mcp --api-url http://localhost:8080 --api-key sk_...
//!
//! # With a spec file (no running API needed)
//! scrapix-mcp --spec-file ./docs/openapi.json --api-url https://scrapix.meilisearch.dev
//! ```

use clap::Parser;
use rmcp::ServiceExt;
use tracing::info;

/// Scrapix MCP Server — exposes crawl API as MCP tools
#[derive(Parser, Debug)]
#[command(name = "scrapix-mcp")]
#[command(version, about = "MCP server for Scrapix API")]
struct Args {
    /// Scrapix API base URL
    #[arg(long, env = "SCRAPIX_API_URL", default_value = "http://localhost:8080")]
    api_url: String,

    /// API key for authentication
    #[arg(long, env = "SCRAPIX_API_KEY")]
    api_key: Option<String>,

    /// Path to a local OpenAPI spec file (instead of fetching from the API)
    #[arg(long, env = "SCRAPIX_SPEC_FILE")]
    spec_file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    // Load OpenAPI spec (from file or remote API)
    let spec_json: serde_json::Value = if let Some(ref path) = args.spec_file {
        let contents = tokio::fs::read_to_string(path).await?;
        serde_json::from_str(&contents)?
    } else {
        let spec_url = format!("{}/openapi.json", args.api_url.trim_end_matches('/'));
        info!(spec_url = %spec_url, "Fetching OpenAPI spec from API");
        let resp = reqwest::get(&spec_url).await?;
        if !resp.status().is_success() {
            return Err(format!(
                "Failed to fetch OpenAPI spec from {}: {}",
                spec_url,
                resp.status()
            )
            .into());
        }
        resp.json().await?
    };

    // Build default headers with API key if provided
    let default_headers = if let Some(ref api_key) = args.api_key {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::HeaderName::from_static("x-api-key"),
            reqwest::header::HeaderValue::from_str(api_key)?,
        );
        Some(headers)
    } else {
        None
    };

    let base_url: url::Url = args.api_url.parse()?;

    // Build the MCP server from the OpenAPI spec
    let mut server = rmcp_openapi::Server::new(
        spec_json,
        base_url,
        default_headers,
        None,  // no filters
        false, // include tool descriptions
        false, // include parameter descriptions
    );
    server.load_openapi_spec()?;

    info!(
        tools = server.tool_count(),
        "MCP server ready with {} tools",
        server.tool_count()
    );

    // Start stdio transport (default for Claude Code, Cursor, etc.)
    let transport = rmcp::transport::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
