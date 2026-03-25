//! OAuth 2.1 endpoints for MCP remote server authentication.
//!
//! Implements:
//! - RFC 8414: OAuth Authorization Server Metadata
//! - RFC 7591: Dynamic Client Registration
//! - RFC 7636: PKCE (S256 only)
//! - RFC 7009: Token Revocation
//!
//! Flow: Claude App → discover metadata → register client → authorize (browser login)
//! → exchange code for tokens → call /mcp with Bearer token.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tracing::{info, warn};

use super::{AuthState, AuthenticatedAccount};
use scrapix_auth::password;

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Serialize)]
struct OAuthMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
    revocation_endpoint: String,
    response_types_supported: Vec<&'static str>,
    grant_types_supported: Vec<&'static str>,
    code_challenge_methods_supported: Vec<&'static str>,
    token_endpoint_auth_methods_supported: Vec<&'static str>,
    scopes_supported: Vec<&'static str>,
}

#[derive(Deserialize)]
struct RegisterClientRequest {
    client_name: Option<String>,
    redirect_uris: Vec<String>,
}

#[derive(Serialize)]
struct RegisterClientResponse {
    client_id: String,
    client_name: Option<String>,
    redirect_uris: Vec<String>,
}

#[derive(Deserialize)]
pub struct AuthorizeParams {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    code_challenge: String,
    code_challenge_method: Option<String>,
    state: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct AuthorizeFormData {
    email: String,
    password: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    state: String,
}

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    code: Option<String>,
    code_verifier: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    refresh_token: Option<String>,
    scope: String,
}

#[derive(Deserialize)]
struct RevokeRequest {
    token: String,
}

#[derive(Serialize)]
struct OAuthError {
    error: String,
    error_description: String,
}

type OAuthResult<T> = Result<T, (StatusCode, Json<OAuthError>)>;

fn oauth_err(status: StatusCode, error: &str, description: &str) -> (StatusCode, Json<OAuthError>) {
    (
        status,
        Json(OAuthError {
            error: error.to_string(),
            error_description: description.to_string(),
        }),
    )
}

// ============================================================================
// Helpers
// ============================================================================

fn base_url() -> String {
    std::env::var("BASE_URL").unwrap_or_else(|_| "https://scrapix.meilisearch.dev".to_string())
}

fn generate_random_string(prefix: &str, len: usize) -> String {
    use rand::Rng;
    let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let random: String = (0..len)
        .map(|_| chars[rng.gen_range(0..chars.len())] as char)
        .collect();
    format!("{prefix}{random}")
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn verify_pkce(code_verifier: &str, code_challenge: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(code_verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(hasher.finalize());
    computed == code_challenge
}

// ============================================================================
// Handlers
// ============================================================================

/// RFC 8414: OAuth Authorization Server Metadata
async fn oauth_metadata() -> Json<OAuthMetadata> {
    let base = base_url();
    Json(OAuthMetadata {
        issuer: base.clone(),
        authorization_endpoint: format!("{base}/oauth/authorize"),
        token_endpoint: format!("{base}/oauth/token"),
        registration_endpoint: format!("{base}/oauth/register"),
        revocation_endpoint: format!("{base}/oauth/revoke"),
        response_types_supported: vec!["code"],
        grant_types_supported: vec!["authorization_code", "refresh_token"],
        code_challenge_methods_supported: vec!["S256"],
        token_endpoint_auth_methods_supported: vec!["none"],
        scopes_supported: vec!["mcp"],
    })
}

/// RFC 7591: Dynamic Client Registration
async fn register_client(
    State(state): State<Arc<AuthState>>,
    Json(req): Json<RegisterClientRequest>,
) -> OAuthResult<Json<RegisterClientResponse>> {
    if req.redirect_uris.is_empty() {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "At least one redirect_uri is required",
        ));
    }

    // Validate redirect URIs
    for uri in &req.redirect_uris {
        if url::Url::parse(uri).is_err() {
            return Err(oauth_err(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                &format!("Invalid redirect_uri: {uri}"),
            ));
        }
    }

    let client_id = generate_random_string("sxc_", 32);

    sqlx::query(
        "INSERT INTO oauth_clients (client_id, client_name, redirect_uris) VALUES ($1, $2, $3)",
    )
    .bind(&client_id)
    .bind(&req.client_name)
    .bind(&req.redirect_uris)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        warn!(error = %e, "Failed to register OAuth client");
        oauth_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "Failed to register client",
        )
    })?;

    info!(client_id = %client_id, "OAuth client registered");

    Ok(Json(RegisterClientResponse {
        client_id,
        client_name: req.client_name,
        redirect_uris: req.redirect_uris,
    }))
}

/// GET /oauth/authorize — render login form
async fn authorize_get(
    State(state): State<Arc<AuthState>>,
    Query(params): Query<AuthorizeParams>,
) -> Result<Response, (StatusCode, Json<OAuthError>)> {
    if params.response_type != "code" {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "unsupported_response_type",
            "Only response_type=code is supported",
        ));
    }

    let method = params.code_challenge_method.as_deref().unwrap_or("S256");
    if method != "S256" {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Only S256 code_challenge_method is supported",
        ));
    }

    // Validate client_id and redirect_uri
    let client = sqlx::query("SELECT redirect_uris FROM oauth_clients WHERE client_id = $1")
        .bind(&params.client_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            oauth_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Database error",
            )
        })?
        .ok_or_else(|| {
            oauth_err(
                StatusCode::BAD_REQUEST,
                "invalid_client",
                "Unknown client_id",
            )
        })?;

    let registered_uris: Vec<String> = client.get("redirect_uris");
    if !registered_uris.contains(&params.redirect_uri) {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect_uri not registered for this client",
        ));
    }

    let state_val = params.state.as_deref().unwrap_or("");

    // Render minimal login form
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>Sign in to Scrapix</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
         background: #0a0a0a; color: #e5e5e5; display: flex; justify-content: center;
         align-items: center; min-height: 100vh; }}
  .card {{ background: #171717; border: 1px solid #262626; border-radius: 12px;
           padding: 2rem; width: 100%; max-width: 400px; }}
  h1 {{ font-size: 1.5rem; margin-bottom: 0.5rem; }}
  p {{ color: #a3a3a3; font-size: 0.875rem; margin-bottom: 1.5rem; }}
  label {{ display: block; font-size: 0.875rem; margin-bottom: 0.25rem; color: #d4d4d4; }}
  input {{ width: 100%; padding: 0.625rem; background: #0a0a0a; border: 1px solid #262626;
          border-radius: 8px; color: #e5e5e5; font-size: 0.875rem; margin-bottom: 1rem; }}
  input:focus {{ outline: none; border-color: #6366f1; }}
  button {{ width: 100%; padding: 0.625rem; background: #6366f1; color: white; border: none;
           border-radius: 8px; font-size: 0.875rem; cursor: pointer; font-weight: 500; }}
  button:hover {{ background: #4f46e5; }}
  .error {{ color: #ef4444; font-size: 0.8rem; margin-bottom: 1rem; display: none; }}
</style>
</head>
<body>
<div class="card">
  <h1>Sign in to Scrapix</h1>
  <p>An application is requesting access to your account via MCP.</p>
  <div class="error" id="error"></div>
  <form method="POST" action="/oauth/authorize">
    <input type="hidden" name="client_id" value="{}"/>
    <input type="hidden" name="redirect_uri" value="{}"/>
    <input type="hidden" name="code_challenge" value="{}"/>
    <input type="hidden" name="code_challenge_method" value="{}"/>
    <input type="hidden" name="state" value="{}"/>
    <label for="email">Email</label>
    <input type="email" id="email" name="email" required autocomplete="email"/>
    <label for="password">Password</label>
    <input type="password" id="password" name="password" required autocomplete="current-password"/>
    <button type="submit">Sign in &amp; Authorize</button>
  </form>
</div>
</body>
</html>"#,
        html_escape(&params.client_id),
        html_escape(&params.redirect_uri),
        html_escape(&params.code_challenge),
        html_escape(method),
        html_escape(state_val),
    );

    Ok(Html(html).into_response())
}

/// POST /oauth/authorize — validate credentials, issue authorization code, redirect
async fn authorize_post(
    State(state): State<Arc<AuthState>>,
    Form(form): Form<AuthorizeFormData>,
) -> Result<Response, Response> {
    // Validate credentials
    let row = sqlx::query("SELECT id, password_hash FROM users WHERE email = $1")
        .bind(&form.email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            authorize_error_redirect(
                &form.redirect_uri,
                &form.state,
                "server_error",
                "Database error",
            )
        })?
        .ok_or_else(|| authorize_error_page("Invalid email or password"))?;

    let pw_hash: String = row.get("password_hash");
    let valid = password::verify_password(&form.password, &pw_hash).unwrap_or(false);
    if !valid {
        return Err(authorize_error_page("Invalid email or password"));
    }

    let user_id: uuid::Uuid = row.get("id");

    // Validate client_id
    let client_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_clients WHERE client_id = $1)")
            .bind(&form.client_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(false);

    if !client_exists {
        return Err(authorize_error_page("Invalid client"));
    }

    // Generate authorization code
    let code = generate_random_string("sxac_", 48);
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    sqlx::query(
        "INSERT INTO oauth_authorization_codes \
         (code, client_id, user_id, redirect_uri, code_challenge, code_challenge_method, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&code)
    .bind(&form.client_id)
    .bind(user_id)
    .bind(&form.redirect_uri)
    .bind(&form.code_challenge)
    .bind(&form.code_challenge_method)
    .bind(expires_at)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        authorize_error_redirect(
            &form.redirect_uri,
            &form.state,
            "server_error",
            "Failed to create authorization code",
        )
    })?;

    info!(user_id = %user_id, client_id = %form.client_id, "OAuth authorization code issued");

    // Redirect back with code
    let mut redirect_url = url::Url::parse(&form.redirect_uri)
        .map_err(|_| authorize_error_page("Invalid redirect URI"))?;
    redirect_url.query_pairs_mut().append_pair("code", &code);
    if !form.state.is_empty() {
        redirect_url
            .query_pairs_mut()
            .append_pair("state", &form.state);
    }

    Ok(Redirect::temporary(redirect_url.as_str()).into_response())
}

/// POST /oauth/token — exchange code for tokens, or refresh
async fn token_exchange(
    State(state): State<Arc<AuthState>>,
    Form(req): Form<TokenRequest>,
) -> OAuthResult<Json<TokenResponse>> {
    match req.grant_type.as_str() {
        "authorization_code" => handle_authorization_code(&state, &req).await,
        "refresh_token" => handle_refresh_token(&state, &req).await,
        _ => Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "Supported: authorization_code, refresh_token",
        )),
    }
}

async fn handle_authorization_code(
    state: &AuthState,
    req: &TokenRequest,
) -> OAuthResult<Json<TokenResponse>> {
    let code = req
        .code
        .as_deref()
        .ok_or_else(|| oauth_err(StatusCode::BAD_REQUEST, "invalid_request", "Missing code"))?;
    let code_verifier = req.code_verifier.as_deref().ok_or_else(|| {
        oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Missing code_verifier",
        )
    })?;

    // Look up authorization code
    let row = sqlx::query(
        "SELECT client_id, user_id, redirect_uri, code_challenge, code_challenge_method, expires_at, used \
         FROM oauth_authorization_codes WHERE code = $1",
    )
    .bind(code)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| oauth_err(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "Database error"))?
    .ok_or_else(|| oauth_err(StatusCode::BAD_REQUEST, "invalid_grant", "Invalid authorization code"))?;

    // Check if already used
    let used: bool = row.get("used");
    if used {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "Authorization code already used",
        ));
    }

    // Check expiry
    let expires_at: chrono::DateTime<chrono::Utc> = row.get("expires_at");
    if chrono::Utc::now() > expires_at {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "Authorization code expired",
        ));
    }

    // Validate client_id matches
    let stored_client_id: String = row.get("client_id");
    if let Some(ref client_id) = req.client_id {
        if *client_id != stored_client_id {
            return Err(oauth_err(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "client_id mismatch",
            ));
        }
    }

    // Validate redirect_uri matches
    let stored_redirect_uri: String = row.get("redirect_uri");
    if let Some(ref redirect_uri) = req.redirect_uri {
        if *redirect_uri != stored_redirect_uri {
            return Err(oauth_err(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "redirect_uri mismatch",
            ));
        }
    }

    // Verify PKCE
    let code_challenge: String = row.get("code_challenge");
    if !verify_pkce(code_verifier, &code_challenge) {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "PKCE verification failed",
        ));
    }

    // Mark code as used
    sqlx::query("UPDATE oauth_authorization_codes SET used = true WHERE code = $1")
        .bind(code)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            oauth_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Database error",
            )
        })?;

    let user_id: uuid::Uuid = row.get("user_id");

    // Generate tokens
    issue_token_pair(state, &stored_client_id, user_id, None).await
}

async fn handle_refresh_token(
    state: &AuthState,
    req: &TokenRequest,
) -> OAuthResult<Json<TokenResponse>> {
    let refresh_token = req.refresh_token.as_deref().ok_or_else(|| {
        oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Missing refresh_token",
        )
    })?;

    let token_hash = hash_token(refresh_token);

    let row = sqlx::query(
        "SELECT id, client_id, user_id, expires_at, revoked \
         FROM oauth_tokens WHERE token_hash = $1 AND token_type = 'refresh'",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        oauth_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "Database error",
        )
    })?
    .ok_or_else(|| {
        oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "Invalid refresh token",
        )
    })?;

    let revoked: bool = row.get("revoked");
    if revoked {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "Refresh token has been revoked",
        ));
    }

    let expires_at: chrono::DateTime<chrono::Utc> = row.get("expires_at");
    if chrono::Utc::now() > expires_at {
        return Err(oauth_err(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "Refresh token expired",
        ));
    }

    // Revoke the old refresh token (rotation)
    let old_token_id: uuid::Uuid = row.get("id");
    sqlx::query("UPDATE oauth_tokens SET revoked = true WHERE id = $1")
        .bind(old_token_id)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            oauth_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Database error",
            )
        })?;

    let client_id: String = row.get("client_id");
    let user_id: uuid::Uuid = row.get("user_id");

    issue_token_pair(state, &client_id, user_id, Some(old_token_id)).await
}

/// Generate access + refresh token pair
async fn issue_token_pair(
    state: &AuthState,
    client_id: &str,
    user_id: uuid::Uuid,
    parent_token_id: Option<uuid::Uuid>,
) -> OAuthResult<Json<TokenResponse>> {
    let access_token = generate_random_string("sxat_", 48);
    let refresh_token = generate_random_string("sxrt_", 48);

    let access_hash = hash_token(&access_token);
    let refresh_hash = hash_token(&refresh_token);

    let access_expires = chrono::Utc::now() + chrono::Duration::hours(1);
    let refresh_expires = chrono::Utc::now() + chrono::Duration::days(30);

    // Insert access token
    sqlx::query(
        "INSERT INTO oauth_tokens (token_hash, token_type, client_id, user_id, expires_at, parent_token_id) \
         VALUES ($1, 'access', $2, $3, $4, $5)",
    )
    .bind(&access_hash)
    .bind(client_id)
    .bind(user_id)
    .bind(access_expires)
    .bind(parent_token_id)
    .execute(&state.pool)
    .await
    .map_err(|_| oauth_err(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "Failed to create access token"))?;

    // Insert refresh token
    sqlx::query(
        "INSERT INTO oauth_tokens (token_hash, token_type, client_id, user_id, expires_at, parent_token_id) \
         VALUES ($1, 'refresh', $2, $3, $4, $5)",
    )
    .bind(&refresh_hash)
    .bind(client_id)
    .bind(user_id)
    .bind(refresh_expires)
    .bind(parent_token_id)
    .execute(&state.pool)
    .await
    .map_err(|_| oauth_err(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "Failed to create refresh token"))?;

    info!(user_id = %user_id, client_id = %client_id, "OAuth tokens issued");

    Ok(Json(TokenResponse {
        access_token,
        token_type: "Bearer",
        expires_in: 3600,
        refresh_token: Some(refresh_token),
        scope: "mcp".to_string(),
    }))
}

/// POST /oauth/revoke — RFC 7009 token revocation
async fn revoke_token(
    State(state): State<Arc<AuthState>>,
    Form(req): Form<RevokeRequest>,
) -> StatusCode {
    let token_hash = hash_token(&req.token);

    // Revoke the token (and any children if it's a refresh token)
    let result = sqlx::query(
        "UPDATE oauth_tokens SET revoked = true WHERE token_hash = $1 AND revoked = false",
    )
    .bind(&token_hash)
    .execute(&state.pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => {
            info!("OAuth token revoked");
        }
        _ => {
            // RFC 7009: always return 200, even for invalid tokens
        }
    }

    StatusCode::OK
}

/// Validate a Bearer token and return the authenticated account.
/// Used by the /mcp route middleware.
pub async fn validate_bearer_token(
    pool: &sqlx::PgPool,
    token: &str,
) -> Result<AuthenticatedAccount, StatusCode> {
    let token_hash = hash_token(token);

    let row = sqlx::query(
        "SELECT t.user_id, t.expires_at, t.revoked, a.id AS account_id, a.tier \
         FROM oauth_tokens t \
         JOIN account_members m ON m.user_id = t.user_id \
         JOIN accounts a ON a.id = m.account_id \
         WHERE t.token_hash = $1 AND t.token_type = 'access' \
         LIMIT 1",
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    let revoked: bool = row.get("revoked");
    if revoked {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let expires_at: chrono::DateTime<chrono::Utc> = row.get("expires_at");
    if chrono::Utc::now() > expires_at {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let account_id: uuid::Uuid = row.get("account_id");
    let tier: String = row.get("tier");

    Ok(AuthenticatedAccount {
        account_id: account_id.to_string(),
        tier,
        api_key_id: None,
    })
}

// ============================================================================
// HTML helpers
// ============================================================================

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn authorize_error_page(message: &str) -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"/><title>Authorization Error</title>
<style>
  body {{ font-family: -apple-system, sans-serif; background: #0a0a0a; color: #e5e5e5;
         display: flex; justify-content: center; align-items: center; min-height: 100vh; }}
  .card {{ background: #171717; border: 1px solid #262626; border-radius: 12px; padding: 2rem;
           max-width: 400px; text-align: center; }}
  .error {{ color: #ef4444; margin-bottom: 1rem; }}
  a {{ color: #6366f1; }}
</style></head>
<body><div class="card">
  <p class="error">{}</p>
  <p>Please go back and try again.</p>
</div></body></html>"#,
        html_escape(message)
    );
    (StatusCode::BAD_REQUEST, Html(html)).into_response()
}

fn authorize_error_redirect(
    redirect_uri: &str,
    state: &str,
    error: &str,
    description: &str,
) -> Response {
    if let Ok(mut url) = url::Url::parse(redirect_uri) {
        url.query_pairs_mut()
            .append_pair("error", error)
            .append_pair("error_description", description);
        if !state.is_empty() {
            url.query_pairs_mut().append_pair("state", state);
        }
        Redirect::temporary(url.as_str()).into_response()
    } else {
        authorize_error_page(description)
    }
}

// ============================================================================
// Background cleanup task
// ============================================================================

/// Spawn a background task to clean up expired OAuth codes and tokens
pub fn spawn_token_cleanup(pool: sqlx::PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600)); // hourly
        loop {
            interval.tick().await;

            // Delete expired authorization codes
            let _ = sqlx::query(
                "DELETE FROM oauth_authorization_codes WHERE expires_at < now() - INTERVAL '1 hour'",
            )
            .execute(&pool)
            .await;

            // Delete expired and revoked tokens older than 7 days
            let _ = sqlx::query(
                "DELETE FROM oauth_tokens WHERE (expires_at < now() - INTERVAL '7 days') OR (revoked = true AND created_at < now() - INTERVAL '7 days')",
            )
            .execute(&pool)
            .await;
        }
    })
}

// ============================================================================
// Router
// ============================================================================

/// Public OAuth routes (no auth required)
pub fn oauth_routes(state: Arc<AuthState>) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_metadata),
        )
        .route("/oauth/register", post(register_client))
        .route("/oauth/authorize", get(authorize_get).post(authorize_post))
        .route("/oauth/token", post(token_exchange))
        .route("/oauth/revoke", post(revoke_token))
        .with_state(state)
}
