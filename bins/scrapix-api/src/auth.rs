//! API Key Authentication Middleware
//!
//! This module provides middleware for authenticating API requests using
//! API keys stored in Supabase. Keys are hashed with SHA-256 before storage.
//!
//! ## Usage
//!
//! Add the middleware to routes that require authentication:
//!
//! ```rust,ignore
//! let protected = Router::new()
//!     .route("/scrape", post(scrape_url))
//!     .route("/crawl", post(create_crawl))
//!     .layer(middleware::from_fn_with_state(pool, validate_api_key));
//! ```

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::sync::Arc;
use tracing::{debug, warn};

/// Account information extracted from a validated API key
#[derive(Debug, Clone)]
pub struct AuthenticatedAccount {
    pub account_id: String,
    pub tier: String,
}

/// Error response for authentication failures
#[derive(Debug, Serialize)]
pub(crate) struct AuthError {
    error: String,
    code: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, Json(self)).into_response()
    }
}

/// State for authentication middleware
pub struct AuthState {
    pub pool: PgPool,
}

impl AuthState {
    /// Create a new AuthState by connecting to the database
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }
}

/// Hash an API key using SHA-256
fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// Middleware to validate API key from X-API-Key header
///
/// This middleware:
/// 1. Extracts the API key from the X-API-Key header
/// 2. Hashes the key with SHA-256
/// 3. Looks up the hash in the database via the validate_api_key function
/// 4. If valid, stores the account info in request extensions
/// 5. If invalid, returns a 401 Unauthorized response
pub async fn validate_api_key(
    State(auth_state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    // Extract API key from header
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AuthError {
            error: "Missing API key".to_string(),
            code: "missing_api_key".to_string(),
        })?;

    // Validate key format (must start with sk_live_ or sk_test_)
    if !api_key.starts_with("sk_live_") && !api_key.starts_with("sk_test_") {
        return Err(AuthError {
            error: "Invalid API key format".to_string(),
            code: "invalid_api_key".to_string(),
        });
    }

    // Hash the key
    let key_hash = hash_api_key(api_key);
    debug!(prefix = %api_key.get(..12).unwrap_or("???"), "Validating API key");

    // Look up in database using the validate_api_key function
    let result = sqlx::query(
        "SELECT account_id, tier, active FROM validate_api_key($1)"
    )
    .bind(&key_hash)
    .fetch_optional(&auth_state.pool)
    .await
    .map_err(|e| {
        warn!(error = %e, "Database error during API key validation");
        AuthError {
            error: "Authentication service unavailable".to_string(),
            code: "auth_service_error".to_string(),
        }
    })?;

    let row = result.ok_or_else(|| {
        debug!("API key not found or inactive");
        AuthError {
            error: "Invalid or inactive API key".to_string(),
            code: "invalid_api_key".to_string(),
        }
    })?;

    let account_id: String = row.try_get("account_id").map_err(|_| AuthError {
        error: "Invalid API key".to_string(),
        code: "invalid_api_key".to_string(),
    })?;

    let tier: String = row.try_get("tier").map_err(|_| AuthError {
        error: "Invalid API key".to_string(),
        code: "invalid_api_key".to_string(),
    })?;

    let active: bool = row.try_get("active").unwrap_or(false);
    if !active {
        return Err(AuthError {
            error: "Account is inactive".to_string(),
            code: "account_inactive".to_string(),
        });
    }

    debug!(
        account_id = %account_id,
        tier = %tier,
        "API key validated successfully"
    );

    // Store account info in request extensions for handlers to use
    request.extensions_mut().insert(AuthenticatedAccount {
        account_id,
        tier,
    });

    Ok(next.run(request).await)
}

/// Extract authenticated account from request extensions
///
/// Use this in handlers to get the current account:
///
/// ```rust,ignore
/// async fn my_handler(
///     Extension(account): Extension<AuthenticatedAccount>,
/// ) -> impl IntoResponse {
///     // account.account_id is available here
/// }
/// ```
pub fn get_authenticated_account(request: &Request) -> Option<&AuthenticatedAccount> {
    request.extensions().get::<AuthenticatedAccount>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_api_key() {
        let key = "sk_live_test123";
        let hash = hash_api_key(key);

        // SHA-256 should produce a 64-character hex string
        assert_eq!(hash.len(), 64);

        // Same input should produce same output
        assert_eq!(hash_api_key(key), hash);

        // Different input should produce different output
        assert_ne!(hash_api_key("sk_live_other"), hash);
    }
}
