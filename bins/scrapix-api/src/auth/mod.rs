//! Authentication module
//!
//! Provides password-based auth with JWT sessions and API key validation.
//! Core auth primitives (JWT, password hashing, types) are in `scrapix-auth`.

pub(crate) mod handlers;
pub(crate) mod middleware;
pub(crate) mod oauth;
pub(crate) mod social;

// Re-export core auth primitives from the scrapix-auth crate.
// Local jwt/password modules are no longer needed — use the crate directly.
pub use scrapix_auth::{AuthenticatedAccount, AuthenticatedUser, Claims};

pub use handlers::auth_routes;
pub(crate) use handlers::get_user_account_id;
pub use handlers::session_routes;
pub(crate) use middleware::validate_api_key_or_session;
pub(crate) use middleware::validate_session;
pub use oauth::oauth_routes;
pub use social::{
    social_auth_routes, OAuthStateStore, ProviderConfig, SocialAuthState, SocialOAuthConfig,
};

use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::email::EmailClient;

/// Shared auth state: database pool + JWT secret
#[derive(Clone)]
pub struct AuthState {
    pub pool: PgPool,
    pub jwt_secret: String,
    pub email_client: Option<EmailClient>,
}

impl AuthState {
    pub async fn new(database_url: &str, jwt_secret: String) -> Result<Self, sqlx::Error> {
        // Heroku Postgres requires SSL but doesn't include sslmode in DATABASE_URL.
        // Append sslmode=require if no sslmode is already specified.
        let url = if !database_url.contains("sslmode=") {
            let sep = if database_url.contains('?') { "&" } else { "?" };
            format!("{database_url}{sep}sslmode=require")
        } else {
            database_url.to_string()
        };
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&url)
            .await?;
        Ok(Self {
            pool,
            jwt_secret,
            email_client: None,
        })
    }
}
