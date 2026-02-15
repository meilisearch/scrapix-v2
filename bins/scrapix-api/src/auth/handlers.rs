use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tracing::info;

use super::{jwt, password, AuthState, AuthenticatedUser};

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Deserialize)]
pub struct SignupRequest {
    email: String,
    password: String,
    full_name: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct UserResponse {
    id: String,
    email: String,
    full_name: Option<String>,
    account: Option<AccountResponse>,
}

#[derive(Serialize)]
struct AccountResponse {
    id: String,
    name: String,
    tier: String,
    active: bool,
    role: String,
}

#[derive(Deserialize)]
pub struct UpdateMeRequest {
    full_name: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateAccountRequest {
    name: Option<String>,
}

#[derive(Serialize)]
struct ApiKeyResponse {
    id: String,
    name: String,
    prefix: String,
    active: bool,
    last_used_at: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    name: String,
}

#[derive(Serialize)]
struct CreatedApiKeyResponse {
    id: String,
    name: String,
    prefix: String,
    key: String,
}

#[derive(Serialize)]
struct BillingResponse {
    tier: String,
    stripe_customer_id: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateBillingRequest {
    tier: String,
}

#[derive(Serialize)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    code: String,
}

type ApiError = (StatusCode, Json<ErrorBody>);

// ============================================================================
// Helpers
// ============================================================================

fn build_session_cookie(token: String) -> Cookie<'static> {
    Cookie::build(("scrapix_session", token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build()
}

fn clear_session_cookie() -> Cookie<'static> {
    Cookie::build(("scrapix_session", ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::ZERO)
        .build()
}

fn err(status: StatusCode, msg: &str, code: &str) -> ApiError {
    (
        status,
        Json(ErrorBody {
            error: msg.to_string(),
            code: code.to_string(),
        }),
    )
}

/// Get the user's account_id via account_members
async fn get_user_account_id(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
) -> Result<uuid::Uuid, StatusCode> {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT account_id FROM account_members WHERE user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)
}

// ============================================================================
// Auth handlers (no auth required)
// ============================================================================

async fn signup(
    State(state): State<Arc<AuthState>>,
    jar: CookieJar,
    Json(req): Json<SignupRequest>,
) -> Result<(CookieJar, Json<UserResponse>), ApiError> {
    if req.email.is_empty() || req.password.len() < 6 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Email required and password must be at least 6 characters",
            "validation_error",
        ));
    }

    // Check if email already taken
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
            .bind(&req.email)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(true);

    if exists {
        return Err(err(
            StatusCode::CONFLICT,
            "Email already registered",
            "email_taken",
        ));
    }

    let pw_hash = password::hash_password(&req.password).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to hash password",
            "internal_error",
        )
    })?;

    // Create user, account, and membership in a transaction
    let mut tx = state.pool.begin().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, full_name) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(&req.email)
    .bind(&pw_hash)
    .bind(&req.full_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create user",
            "internal_error",
        )
    })?;

    let account_name = req
        .full_name
        .as_deref()
        .unwrap_or(&req.email)
        .to_string()
        + "'s Account";

    let account_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO accounts (name) VALUES ($1) RETURNING id")
            .bind(&account_name)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to create account",
                    "internal_error",
                )
            })?;

    sqlx::query("INSERT INTO account_members (user_id, account_id, role) VALUES ($1, $2, 'owner')")
        .bind(user_id)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create membership",
                "internal_error",
            )
        })?;

    tx.commit().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    info!(user_id = %user_id, email = %req.email, "New user signed up");

    let token = jwt::encode_jwt(&user_id, &req.email, &state.jwt_secret).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create session",
            "internal_error",
        )
    })?;

    let jar = jar.add(build_session_cookie(token));

    Ok((
        jar,
        Json(UserResponse {
            id: user_id.to_string(),
            email: req.email,
            full_name: req.full_name,
            account: Some(AccountResponse {
                id: account_id.to_string(),
                name: account_name,
                tier: "free".to_string(),
                active: true,
                role: "owner".to_string(),
            }),
        }),
    ))
}

async fn login(
    State(state): State<Arc<AuthState>>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<(CookieJar, Json<UserResponse>), ApiError> {
    let row = sqlx::query("SELECT id, email, password_hash, full_name FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?
        .ok_or_else(|| {
            err(
                StatusCode::UNAUTHORIZED,
                "Invalid email or password",
                "invalid_credentials",
            )
        })?;

    let user_id: uuid::Uuid = row.get("id");
    let email: String = row.get("email");
    let pw_hash: String = row.get("password_hash");
    let full_name: Option<String> = row.get("full_name");

    let valid = password::verify_password(&req.password, &pw_hash).unwrap_or(false);
    if !valid {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Invalid email or password",
            "invalid_credentials",
        ));
    }

    // Get account
    let account = sqlx::query(
        "SELECT a.id, a.name, a.tier, a.active, m.role \
         FROM account_members m JOIN accounts a ON a.id = m.account_id \
         WHERE m.user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
    .map(|r| AccountResponse {
        id: r.get::<uuid::Uuid, _>("id").to_string(),
        name: r.get("name"),
        tier: r.get("tier"),
        active: r.get("active"),
        role: r.get("role"),
    });

    let token = jwt::encode_jwt(&user_id, &email, &state.jwt_secret).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create session",
            "internal_error",
        )
    })?;

    info!(user_id = %user_id, email = %email, "User logged in");

    let jar = jar.add(build_session_cookie(token));

    Ok((
        jar,
        Json(UserResponse {
            id: user_id.to_string(),
            email,
            full_name,
            account,
        }),
    ))
}

async fn logout(jar: CookieJar) -> (CookieJar, Json<MessageResponse>) {
    let jar = jar.add(clear_session_cookie());
    (jar, Json(MessageResponse { message: "Logged out".to_string() }))
}

// ============================================================================
// Session-protected handlers
// ============================================================================

async fn get_me(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<UserResponse>, ApiError> {
    let row = sqlx::query("SELECT id, email, full_name FROM users WHERE id = $1")
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "User not found", "not_found"))?;

    let account = sqlx::query(
        "SELECT a.id, a.name, a.tier, a.active, m.role \
         FROM account_members m JOIN accounts a ON a.id = m.account_id \
         WHERE m.user_id = $1 LIMIT 1",
    )
    .bind(user.user_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
    .map(|r| AccountResponse {
        id: r.get::<uuid::Uuid, _>("id").to_string(),
        name: r.get("name"),
        tier: r.get("tier"),
        active: r.get("active"),
        role: r.get("role"),
    });

    Ok(Json(UserResponse {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        email: row.get("email"),
        full_name: row.get("full_name"),
        account,
    }))
}

async fn update_me(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdateMeRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    if let Some(ref name) = req.full_name {
        sqlx::query("UPDATE users SET full_name = $1 WHERE id = $2")
            .bind(name)
            .bind(user.user_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update",
                    "internal_error",
                )
            })?;
    }
    Ok(Json(MessageResponse { message: "Updated".to_string() }))
}

async fn get_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<AccountResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT a.id, a.name, a.tier, a.active, m.role \
         FROM account_members m JOIN accounts a ON a.id = m.account_id \
         WHERE m.user_id = $1 LIMIT 1",
    )
    .bind(user.user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    Ok(Json(AccountResponse {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        name: row.get("name"),
        tier: row.get("tier"),
        active: row.get("active"),
        role: row.get("role"),
    }))
}

async fn update_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    if let Some(ref name) = req.name {
        sqlx::query("UPDATE accounts SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(account_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update",
                    "internal_error",
                )
            })?;
    }
    Ok(Json(MessageResponse { message: "Updated".to_string() }))
}

async fn list_api_keys(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<ApiKeyResponse>>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let rows = sqlx::query(
        "SELECT id, name, prefix, active, last_used_at, created_at \
         FROM api_keys WHERE account_id = $1 ORDER BY created_at DESC",
    )
    .bind(account_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let keys: Vec<ApiKeyResponse> = rows
        .iter()
        .map(|r| ApiKeyResponse {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            name: r.get("name"),
            prefix: r.get("prefix"),
            active: r.get("active"),
            last_used_at: r
                .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_used_at")
                .map(|t| t.to_rfc3339()),
            created_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        })
        .collect();

    Ok(Json(keys))
}

async fn create_api_key(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<CreatedApiKeyResponse>, ApiError> {
    if req.name.trim().is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Name is required",
            "validation_error",
        ));
    }

    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Generate key server-side (scoped to drop !Send ThreadRng before await)
    let (api_key, prefix, key_hash) = {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let random_part: String = (0..32)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
            .collect();
        let api_key = format!("sk_live_{}", random_part);
        let prefix = format!("{}...", &api_key[..12]);

        let mut hasher = Sha256::new();
        hasher.update(api_key.as_bytes());
        let key_hash = hex::encode(hasher.finalize());
        (api_key, prefix, key_hash)
    };

    let key_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO api_keys (account_id, name, prefix, key_hash) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(account_id)
    .bind(req.name.trim())
    .bind(&prefix)
    .bind(&key_hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create key",
            "internal_error",
        )
    })?;

    info!(key_id = %key_id, account_id = %account_id, "API key created");

    Ok(Json(CreatedApiKeyResponse {
        id: key_id.to_string(),
        name: req.name,
        prefix,
        key: api_key,
    }))
}

async fn revoke_api_key(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(key_id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let key_uuid: uuid::Uuid = key_id.parse().map_err(|_| {
        err(StatusCode::BAD_REQUEST, "Invalid key ID", "validation_error")
    })?;

    let result = sqlx::query(
        "UPDATE api_keys SET active = false WHERE id = $1 AND account_id = $2",
    )
    .bind(key_uuid)
    .bind(account_id)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to revoke key",
            "internal_error",
        )
    })?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Key not found", "not_found"));
    }

    Ok(Json(MessageResponse {
        message: "Key revoked".to_string(),
    }))
}

async fn get_billing(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<BillingResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let row = sqlx::query("SELECT tier, stripe_customer_id FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    Ok(Json(BillingResponse {
        tier: row.get("tier"),
        stripe_customer_id: row.get("stripe_customer_id"),
    }))
}

async fn update_billing(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<UpdateBillingRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let valid_tiers = ["free", "starter", "pro", "enterprise"];
    if !valid_tiers.contains(&req.tier.as_str()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Invalid tier",
            "validation_error",
        ));
    }

    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    sqlx::query("UPDATE accounts SET tier = $1 WHERE id = $2")
        .bind(&req.tier)
        .bind(account_id)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update",
                "internal_error",
            )
        })?;

    Ok(Json(MessageResponse {
        message: "Tier updated".to_string(),
    }))
}

// ============================================================================
// Router constructors
// ============================================================================

/// Routes that require no authentication (signup, login, logout)
pub fn auth_routes(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .with_state(state)
}

/// Routes protected by JWT session cookie
pub fn session_routes(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/auth/me", get(get_me).patch(update_me))
        .route("/account", get(get_account).patch(update_account))
        .route("/account/api-keys", get(list_api_keys).post(create_api_key))
        .route("/account/api-keys/{id}", patch(revoke_api_key))
        .route("/account/billing", get(get_billing).patch(update_billing))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            super::validate_session,
        ))
        .with_state(state)
}
