use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::CookieJar;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tracing::{debug, warn};

use super::{jwt, AuthState, AuthenticatedAccount, AuthenticatedUser};

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

/// Hash an API key using SHA-256
fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Middleware: validate API key from X-API-Key header
pub async fn validate_api_key(
    State(auth_state): State<Arc<AuthState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AuthError {
            error: "Missing API key".to_string(),
            code: "missing_api_key".to_string(),
        })?;

    if !api_key.starts_with("sk_live_") && !api_key.starts_with("sk_test_") {
        return Err(AuthError {
            error: "Invalid API key format".to_string(),
            code: "invalid_api_key".to_string(),
        });
    }

    let key_hash = hash_api_key(api_key);
    debug!(prefix = %api_key.get(..12).unwrap_or("???"), "Validating API key");

    let result = sqlx::query("SELECT account_id, tier, active FROM validate_api_key($1)")
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

    let row = result.ok_or_else(|| AuthError {
        error: "Invalid or inactive API key".to_string(),
        code: "invalid_api_key".to_string(),
    })?;

    let account_id: uuid::Uuid = row.try_get("account_id").map_err(|_| AuthError {
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

    debug!(account_id = %account_id, tier = %tier, "API key validated");

    request.extensions_mut().insert(AuthenticatedAccount {
        account_id: account_id.to_string(),
        tier,
    });

    Ok(next.run(request).await)
}

/// Middleware: validate JWT session from scrapix_session cookie
pub async fn validate_session(
    State(auth_state): State<Arc<AuthState>>,
    jar: CookieJar,
    mut request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    let token = jar
        .get("scrapix_session")
        .map(|c| c.value().to_string())
        .ok_or_else(|| AuthError {
            error: "Not authenticated".to_string(),
            code: "not_authenticated".to_string(),
        })?;

    let claims = jwt::decode_jwt(&token, &auth_state.jwt_secret).map_err(|_| AuthError {
        error: "Invalid or expired session".to_string(),
        code: "invalid_session".to_string(),
    })?;

    let user_id: uuid::Uuid = claims.sub.parse().map_err(|_| AuthError {
        error: "Invalid session".to_string(),
        code: "invalid_session".to_string(),
    })?;

    request.extensions_mut().insert(AuthenticatedUser {
        user_id,
        email: claims.email,
    });

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_api_key() {
        let key = "sk_live_test123";
        let hash = hash_api_key(key);
        assert_eq!(hash.len(), 64);
        assert_eq!(hash_api_key(key), hash);
        assert_ne!(hash_api_key("sk_live_other"), hash);
    }
}
