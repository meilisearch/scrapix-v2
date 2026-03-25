//! Social OAuth login (Google, GitHub).
//!
//! Flow:
//! 1. `GET /auth/social/:provider` — redirect to provider consent screen
//! 2. Provider redirects back to `GET /auth/social/:provider/callback`
//! 3. Exchange code for access token, fetch user profile
//! 4. Find-or-create user, issue session cookie, redirect to dashboard

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use dashmap::DashMap;
use serde::Deserialize;
use sqlx::Row;
use tracing::{info, warn};

use super::AuthState;
use crate::auth::handlers::build_session_cookie;
use scrapix_auth::jwt;

// ============================================================================
// Configuration
// ============================================================================

/// OAuth provider configuration (client credentials).
#[derive(Debug, Clone)]
pub struct SocialOAuthConfig {
    pub google: Option<ProviderConfig>,
    pub github: Option<ProviderConfig>,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub client_id: String,
    pub client_secret: String,
}

/// In-memory state store for CSRF protection (state param).
/// Entries expire after 10 minutes.
#[derive(Clone, Default)]
pub struct OAuthStateStore {
    // state_token -> (provider, redirect_uri, created_at)
    states: Arc<DashMap<String, (String, String, Instant)>>,
}

impl OAuthStateStore {
    pub fn new() -> Self {
        Self {
            states: Arc::new(DashMap::new()),
        }
    }

    fn insert(&self, state: String, provider: String, redirect_uri: String) {
        // Evict expired entries
        let now = Instant::now();
        self.states
            .retain(|_, v| now.duration_since(v.2) < Duration::from_secs(600));
        self.states.insert(state, (provider, redirect_uri, now));
    }

    fn take(&self, state: &str) -> Option<(String, String)> {
        let (_, entry) = self.states.remove(state)?;
        // Check expiry
        if Instant::now().duration_since(entry.2) > Duration::from_secs(600) {
            return None;
        }
        Some((entry.0, entry.1))
    }
}

// ============================================================================
// Shared state for social auth
// ============================================================================

#[derive(Clone)]
pub struct SocialAuthState {
    pub auth: Arc<AuthState>,
    pub config: SocialOAuthConfig,
    pub state_store: OAuthStateStore,
    pub http_client: reqwest::Client,
    /// Base URL for building callback URIs (e.g., "https://api.scrapix.meilisearch.com")
    pub api_base_url: String,
    /// Where to redirect after successful login (e.g., "https://scrapix.meilisearch.com")
    pub console_url: String,
}

// ============================================================================
// Provider user info
// ============================================================================

struct SocialUserInfo {
    email: String,
    name: Option<String>,
    provider_user_id: String,
}

// ============================================================================
// Routes
// ============================================================================

pub fn social_auth_routes(state: SocialAuthState) -> Router {
    Router::new()
        .route("/auth/social/{provider}", get(initiate))
        .route("/auth/social/{provider}/callback", get(callback))
        .with_state(state)
}

// ============================================================================
// GET /auth/social/:provider — redirect to consent screen
// ============================================================================

async fn initiate(State(state): State<SocialAuthState>, Path(provider): Path<String>) -> Response {
    let config = match provider.as_str() {
        "google" => match &state.config.google {
            Some(c) => c,
            None => return error_redirect(&state.console_url, "Google login is not configured"),
        },
        "github" => match &state.config.github {
            Some(c) => c,
            None => return error_redirect(&state.console_url, "GitHub login is not configured"),
        },
        _ => return error_redirect(&state.console_url, "Unknown provider"),
    };

    // Generate CSRF state token
    let state_token = generate_random_token();
    let callback_uri = format!("{}/auth/social/{}/callback", state.api_base_url, provider);
    state
        .state_store
        .insert(state_token.clone(), provider.clone(), callback_uri.clone());

    let auth_url = match provider.as_str() {
        "google" => format!(
            "https://accounts.google.com/o/oauth2/v2/auth?\
             client_id={}&redirect_uri={}&response_type=code&\
             scope=openid%20email%20profile&state={}&access_type=online&prompt=select_account",
            urlencoding::encode(&config.client_id),
            urlencoding::encode(&callback_uri),
            urlencoding::encode(&state_token),
        ),
        "github" => format!(
            "https://github.com/login/oauth/authorize?\
             client_id={}&redirect_uri={}&scope=user:email&state={}",
            urlencoding::encode(&config.client_id),
            urlencoding::encode(&callback_uri),
            urlencoding::encode(&state_token),
        ),
        _ => unreachable!(),
    };

    Redirect::temporary(&auth_url).into_response()
}

// ============================================================================
// GET /auth/social/:provider/callback — handle provider callback
// ============================================================================

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(state): State<SocialAuthState>,
    Path(provider): Path<String>,
    Query(params): Query<CallbackParams>,
) -> Response {
    // Check for provider-side errors
    if let Some(err) = &params.error {
        warn!(provider = %provider, error = %err, "OAuth provider returned error");
        return error_redirect(&state.console_url, &format!("Login cancelled: {err}"));
    }

    let code = match &params.code {
        Some(c) => c.clone(),
        None => return error_redirect(&state.console_url, "Missing authorization code"),
    };

    let state_token = match &params.state {
        Some(s) => s.clone(),
        None => return error_redirect(&state.console_url, "Missing state parameter"),
    };

    // Validate CSRF state
    let (stored_provider, callback_uri) = match state.state_store.take(&state_token) {
        Some(v) => v,
        None => return error_redirect(&state.console_url, "Invalid or expired state"),
    };

    if stored_provider != provider {
        return error_redirect(&state.console_url, "Provider mismatch");
    }

    let config = match provider.as_str() {
        "google" => state.config.google.as_ref().unwrap(),
        "github" => state.config.github.as_ref().unwrap(),
        _ => return error_redirect(&state.console_url, "Unknown provider"),
    };

    // Exchange code for access token
    let access_token =
        match exchange_code(&state.http_client, &provider, config, &code, &callback_uri).await {
            Ok(t) => t,
            Err(e) => {
                warn!(provider = %provider, error = %e, "Failed to exchange OAuth code");
                return error_redirect(&state.console_url, "Failed to authenticate with provider");
            }
        };

    // Fetch user info
    let user_info = match fetch_user_info(&state.http_client, &provider, &access_token).await {
        Ok(info) => info,
        Err(e) => {
            warn!(provider = %provider, error = %e, "Failed to fetch user info");
            return error_redirect(&state.console_url, "Failed to get profile from provider");
        }
    };

    // Find or create user
    match find_or_create_user(&state.auth, &provider, &user_info).await {
        Ok((user_id, email)) => {
            let token = match jwt::encode_jwt(&user_id, &email, &state.auth.jwt_secret) {
                Ok(t) => t,
                Err(_) => return error_redirect(&state.console_url, "Failed to create session"),
            };

            // Build redirect with Set-Cookie header
            let cookie = build_session_cookie(token);
            let redirect_url = format!("{}/dashboard", state.console_url);

            (
                axum::http::StatusCode::SEE_OTHER,
                [
                    ("location", redirect_url.as_str()),
                    ("set-cookie", &cookie.to_string()),
                ],
            )
                .into_response()
        }
        Err(e) => {
            warn!(provider = %provider, error = %e, "Failed to find/create user");
            error_redirect(&state.console_url, "Failed to create account")
        }
    }
}

// ============================================================================
// Token exchange
// ============================================================================

async fn exchange_code(
    client: &reqwest::Client,
    provider: &str,
    config: &ProviderConfig,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<String> {
    match provider {
        "google" => {
            let resp = client
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("code", code),
                    ("client_id", &config.client_id),
                    ("client_secret", &config.client_secret),
                    ("redirect_uri", redirect_uri),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await?
                .json::<HashMap<String, serde_json::Value>>()
                .await?;

            resp.get("access_token")
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("No access_token in Google response"))
        }
        "github" => {
            let resp = client
                .post("https://github.com/login/oauth/access_token")
                .header("Accept", "application/json")
                .form(&[
                    ("code", code),
                    ("client_id", &config.client_id),
                    ("client_secret", &config.client_secret),
                    ("redirect_uri", redirect_uri),
                ])
                .send()
                .await?
                .json::<HashMap<String, serde_json::Value>>()
                .await?;

            resp.get("access_token")
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("No access_token in GitHub response"))
        }
        _ => Err(anyhow::anyhow!("Unknown provider")),
    }
}

// ============================================================================
// Fetch user info from provider
// ============================================================================

async fn fetch_user_info(
    client: &reqwest::Client,
    provider: &str,
    access_token: &str,
) -> anyhow::Result<SocialUserInfo> {
    match provider {
        "google" => {
            let resp = client
                .get("https://www.googleapis.com/oauth2/v2/userinfo")
                .bearer_auth(access_token)
                .send()
                .await?
                .json::<HashMap<String, serde_json::Value>>()
                .await?;

            let email = resp
                .get("email")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("No email in Google profile"))?
                .to_string();

            let name = resp.get("name").and_then(|v| v.as_str()).map(String::from);
            let provider_user_id = resp
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("No id in Google profile"))?
                .to_string();

            Ok(SocialUserInfo {
                email,
                name,
                provider_user_id,
            })
        }
        "github" => {
            // Get user profile
            let profile = client
                .get("https://api.github.com/user")
                .bearer_auth(access_token)
                .header("User-Agent", "scrapix")
                .send()
                .await?
                .json::<HashMap<String, serde_json::Value>>()
                .await?;

            let provider_user_id = profile
                .get("id")
                .and_then(|v| v.as_i64())
                .map(|id| id.to_string())
                .ok_or_else(|| anyhow::anyhow!("No id in GitHub profile"))?;

            let name = profile
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Email may be private — fetch from /user/emails
            let email = match profile.get("email").and_then(|v| v.as_str()) {
                Some(e) if !e.is_empty() => e.to_string(),
                _ => {
                    #[derive(Deserialize)]
                    struct GhEmail {
                        email: String,
                        primary: bool,
                        verified: bool,
                    }

                    let emails: Vec<GhEmail> = client
                        .get("https://api.github.com/user/emails")
                        .bearer_auth(access_token)
                        .header("User-Agent", "scrapix")
                        .send()
                        .await?
                        .json()
                        .await?;

                    emails
                        .into_iter()
                        .find(|e| e.primary && e.verified)
                        .map(|e| e.email)
                        .ok_or_else(|| anyhow::anyhow!("No verified primary email on GitHub"))?
                }
            };

            Ok(SocialUserInfo {
                email,
                name,
                provider_user_id,
            })
        }
        _ => Err(anyhow::anyhow!("Unknown provider")),
    }
}

// ============================================================================
// Find or create user
// ============================================================================

async fn find_or_create_user(
    auth: &AuthState,
    provider: &str,
    info: &SocialUserInfo,
) -> anyhow::Result<(uuid::Uuid, String)> {
    // 1. Check if OAuth identity already exists
    let existing = sqlx::query(
        "SELECT u.id, u.email FROM oauth_identities oi \
         JOIN users u ON u.id = oi.user_id \
         WHERE oi.provider = $1 AND oi.provider_user_id = $2",
    )
    .bind(provider)
    .bind(&info.provider_user_id)
    .fetch_optional(&auth.pool)
    .await?;

    if let Some(row) = existing {
        let user_id: uuid::Uuid = row.get("id");
        let email: String = row.get("email");
        info!(user_id = %user_id, provider = %provider, "Social login: existing identity");
        return Ok((user_id, email));
    }

    // 2. Check if a user with this email already exists → link identity
    let existing_user = sqlx::query("SELECT id, email FROM users WHERE email = $1")
        .bind(&info.email)
        .fetch_optional(&auth.pool)
        .await?;

    if let Some(row) = existing_user {
        let user_id: uuid::Uuid = row.get("id");
        let email: String = row.get("email");

        // Link the OAuth identity
        sqlx::query(
            "INSERT INTO oauth_identities (user_id, provider, provider_user_id) \
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(provider)
        .bind(&info.provider_user_id)
        .execute(&auth.pool)
        .await?;

        // Mark email as verified (provider already verified it)
        sqlx::query(
            "UPDATE users SET email_verified = true WHERE id = $1 AND email_verified = false",
        )
        .bind(user_id)
        .execute(&auth.pool)
        .await?;

        info!(user_id = %user_id, provider = %provider, "Social login: linked to existing user");
        return Ok((user_id, email));
    }

    // 3. Create new user + account + membership (same pattern as signup handler)
    let mut tx = auth.pool.begin().await?;

    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, full_name, email_verified) \
         VALUES ($1, $2, true) RETURNING id",
    )
    .bind(&info.email)
    .bind(&info.name)
    .fetch_one(&mut *tx)
    .await?;

    let account_name = info.name.as_deref().unwrap_or(&info.email).to_string() + "'s Account";

    let account_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO accounts (name) VALUES ($1) RETURNING id")
            .bind(&account_name)
            .fetch_one(&mut *tx)
            .await?;

    sqlx::query("INSERT INTO account_members (user_id, account_id, role) VALUES ($1, $2, 'owner')")
        .bind(user_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

    // Initial credit deposit
    sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'initial_deposit', 100, 100, 'Welcome credit deposit')",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;

    // Link OAuth identity
    sqlx::query(
        "INSERT INTO oauth_identities (user_id, provider, provider_user_id) \
         VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(provider)
    .bind(&info.provider_user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    info!(user_id = %user_id, provider = %provider, email = %info.email, "Social login: new user created");

    // Auto-accept pending invites (same as signup)
    let pending_invites = sqlx::query(
        "SELECT id, account_id, role FROM account_invites \
         WHERE email = $1 AND status = 'pending' AND expires_at > now()",
    )
    .bind(&info.email)
    .fetch_all(&auth.pool)
    .await
    .unwrap_or_default();

    for invite_row in &pending_invites {
        let invite_id: uuid::Uuid = invite_row.get("id");
        let inv_account_id: uuid::Uuid = invite_row.get("account_id");
        let inv_role: String = invite_row.get("role");

        let _ = sqlx::query(
            "INSERT INTO account_members (user_id, account_id, role) VALUES ($1, $2, $3) \
             ON CONFLICT (user_id, account_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(inv_account_id)
        .bind(&inv_role)
        .execute(&auth.pool)
        .await;

        let _ = sqlx::query("UPDATE account_invites SET status = 'accepted' WHERE id = $1")
            .bind(invite_id)
            .execute(&auth.pool)
            .await;

        info!(user_id = %user_id, account_id = %inv_account_id, role = %inv_role, "Auto-accepted pending invite on social signup");
    }

    // Send welcome email
    if let Some(ref mailer) = auth.email_client {
        let name = info.name.as_deref().unwrap_or("");
        mailer.send_welcome(&info.email, name);
    }

    Ok((user_id, info.email.clone()))
}

// ============================================================================
// Helpers
// ============================================================================

fn generate_random_token() -> String {
    use rand::Rng;
    let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| chars[rng.gen_range(0..chars.len())] as char)
        .collect()
}

fn error_redirect(console_url: &str, message: &str) -> Response {
    let url = format!(
        "{}/login?error={}",
        console_url,
        urlencoding::encode(message)
    );
    Redirect::temporary(&url).into_response()
}
