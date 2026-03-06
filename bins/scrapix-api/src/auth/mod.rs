//! Authentication module
//!
//! Provides password-based auth with JWT sessions and API key validation.

mod handlers;
mod jwt;
mod middleware;
mod password;

pub use handlers::auth_routes;
pub(crate) use handlers::get_user_account_id;
pub use handlers::session_routes;
pub(crate) use middleware::validate_api_key_or_session;
pub(crate) use middleware::validate_session;

use sqlx::{postgres::PgPoolOptions, PgPool};

/// Account information extracted from a validated API key
#[derive(Debug, Clone)]
pub struct AuthenticatedAccount {
    pub account_id: String,
    pub tier: String,
}

/// User information extracted from a validated JWT session
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: uuid::Uuid,
    pub email: String,
}

/// Shared auth state: database pool + JWT secret
#[derive(Clone)]
pub struct AuthState {
    pub pool: PgPool,
    pub jwt_secret: String,
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
        Ok(Self { pool, jwt_secret })
    }
}
