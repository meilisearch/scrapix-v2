//! MCP Streamable HTTP transport handler.
//!
//! Embeds the MCP server (same as `scrapix-mcp` binary) into the API process,
//! exposed at `/mcp` with OAuth 2.1 Bearer token authentication.
//! Tool calls are proxied to the local API — no separate API key needed.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, tower::StreamableHttpService, StreamableHttpServerConfig,
};
use serde::Serialize;
use tracing::info;

use crate::auth::{oauth, AuthState};

#[derive(Debug, Serialize)]
struct McpAuthError {
    error: String,
    code: String,
}

/// Middleware: validate Bearer token on /mcp requests.
/// Extracts `AuthenticatedAccount` from the OAuth access token.
pub async fn validate_mcp_bearer(
    State(auth_state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(McpAuthError {
                    error: "Missing Bearer token".to_string(),
                    code: "missing_token".to_string(),
                }),
            )
                .into_response()
        })?
        .to_string();

    let account = oauth::validate_bearer_token(&auth_state.pool, &token)
        .await
        .map_err(|status| {
            (
                status,
                Json(McpAuthError {
                    error: "Invalid or expired token".to_string(),
                    code: "invalid_token".to_string(),
                }),
            )
                .into_response()
        })?;

    request.extensions_mut().insert(account);
    Ok(next.run(request).await)
}

/// Build the MCP StreamableHttpService from the OpenAPI spec.
///
/// The MCP server generates tools from the OpenAPI spec and proxies calls
/// to the local API at `api_base_url` using the account's credentials.
pub fn build_mcp_service(
    api_base_url: &str,
) -> Result<
    StreamableHttpService<rmcp_openapi::Server, LocalSessionManager>,
    Box<dyn std::error::Error>,
> {
    // Build the spec in-memory from our utoipa-generated OpenAPI
    use utoipa::OpenApi;
    let spec = crate::openapi::ScrapixApi::openapi();
    let spec_json: serde_json::Value =
        serde_json::from_str(&spec.to_json().expect("OpenAPI JSON serialization"))?;

    let base_url: url::Url = api_base_url.parse()?;

    // Pre-validate the spec loads correctly
    let mut test_server = rmcp_openapi::Server::new(
        spec_json.clone(),
        base_url.clone(),
        None,
        None,
        false,
        false,
    );
    test_server.load_openapi_spec()?;
    let tool_count = test_server.tool_count();

    let service = StreamableHttpService::new(
        move || {
            let mut server = rmcp_openapi::Server::new(
                spec_json.clone(),
                base_url.clone(),
                None,
                None,
                false,
                false,
            );
            server
                .load_openapi_spec()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(server)
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    info!(
        tools = tool_count,
        "MCP HTTP service ready with {tool_count} tools"
    );

    Ok(service)
}
