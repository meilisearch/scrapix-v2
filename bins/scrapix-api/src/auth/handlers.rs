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
use tracing::{error, info};

use super::{jwt, password, AuthState, AuthenticatedUser};

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SignupRequest {
    email: String,
    password: String,
    full_name: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct UserResponse {
    id: String,
    email: String,
    full_name: Option<String>,
    account: Option<AccountResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AccountResponse {
    id: String,
    name: String,
    tier: String,
    active: bool,
    role: String,
    credits_balance: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateMeRequest {
    full_name: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateAccountRequest {
    name: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ApiKeyResponse {
    id: String,
    name: String,
    prefix: String,
    active: bool,
    last_used_at: Option<String>,
    created_at: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateApiKeyRequest {
    name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct CreatedApiKeyResponse {
    id: String,
    name: String,
    prefix: String,
    key: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct BillingResponse {
    tier: String,
    stripe_customer_id: Option<String>,
    credits_balance: i64,
    auto_topup_enabled: bool,
    auto_topup_amount: i64,
    auto_topup_threshold: i64,
    monthly_spend_limit: Option<i64>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateBillingRequest {
    tier: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TopupRequest {
    amount: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AutoTopupRequest {
    enabled: bool,
    amount: Option<i64>,
    threshold: Option<i64>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SpendLimitRequest {
    monthly_spend_limit: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TransactionResponse {
    id: String,
    #[serde(rename = "type")]
    tx_type: String,
    amount: i64,
    balance_after: i64,
    description: Option<String>,
    created_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TransactionsListResponse {
    transactions: Vec<TransactionResponse>,
    total: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TopupResponse {
    credits_balance: i64,
    transaction_id: String,
    message: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct MessageResponse {
    message: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ErrorBody {
    error: String,
    code: String,
}

type ApiError = (StatusCode, Json<ErrorBody>);

// ============================================================================
// Helpers
// ============================================================================

fn build_session_cookie(token: String) -> Cookie<'static> {
    let secure = std::env::var("ENVIRONMENT")
        .map(|e| e != "development")
        .unwrap_or(true);
    Cookie::build(("scrapix_session", token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
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
pub(crate) async fn get_user_account_id(
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

#[utoipa::path(
    post,
    path = "/auth/signup",
    tag = "auth",
    request_body = SignupRequest,
    responses(
        (status = 200, description = "User created successfully", body = UserResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 409, description = "Email already taken", body = ErrorBody),
    )
)]
pub(crate) async fn signup(
    State(state): State<Arc<AuthState>>,
    jar: CookieJar,
    Json(req): Json<SignupRequest>,
) -> Result<(CookieJar, Json<UserResponse>), ApiError> {
    if req.email.is_empty() || req.password.len() < 12 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Email required and password must be at least 12 characters",
            "validation_error",
        ));
    }

    // Check if email already taken
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
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

    let account_name = req.full_name.as_deref().unwrap_or(&req.email).to_string() + "'s Account";

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

    // Log the initial credit deposit transaction
    sqlx::query(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'initial_deposit', 100, 100, 'Welcome credit deposit')",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to log initial deposit",
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
                credits_balance: 100,
            }),
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = UserResponse),
        (status = 401, description = "Invalid credentials", body = ErrorBody),
    )
)]
pub(crate) async fn login(
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
        "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
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
        credits_balance: r.get("credits_balance"),
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

#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    responses(
        (status = 200, description = "Logged out successfully", body = MessageResponse),
    )
)]
pub(crate) async fn logout(jar: CookieJar) -> (CookieJar, Json<MessageResponse>) {
    let jar = jar.add(clear_session_cookie());
    (
        jar,
        Json(MessageResponse {
            message: "Logged out".to_string(),
        }),
    )
}

// ============================================================================
// Session-protected handlers
// ============================================================================

#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Current user info", body = UserResponse),
        (status = 404, description = "User not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_me(
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
        "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
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
        credits_balance: r.get("credits_balance"),
    });

    Ok(Json(UserResponse {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        email: row.get("email"),
        full_name: row.get("full_name"),
        account,
    }))
}

#[utoipa::path(
    patch,
    path = "/auth/me",
    tag = "auth",
    request_body = UpdateMeRequest,
    responses(
        (status = 200, description = "User updated", body = MessageResponse),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_me(
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
    Ok(Json(MessageResponse {
        message: "Updated".to_string(),
    }))
}

#[utoipa::path(
    get,
    path = "/account",
    tag = "auth",
    responses(
        (status = 200, description = "Account details", body = AccountResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<AccountResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
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
        credits_balance: row.get("credits_balance"),
    }))
}

#[utoipa::path(
    patch,
    path = "/account",
    tag = "auth",
    request_body = UpdateAccountRequest,
    responses(
        (status = 200, description = "Account updated", body = MessageResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_account(
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
    Ok(Json(MessageResponse {
        message: "Updated".to_string(),
    }))
}

#[utoipa::path(
    get,
    path = "/account/api-keys",
    tag = "auth",
    responses(
        (status = 200, description = "List of API keys", body = Vec<ApiKeyResponse>),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn list_api_keys(
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

#[utoipa::path(
    post,
    path = "/account/api-keys",
    tag = "auth",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 200, description = "API key created", body = CreatedApiKeyResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn create_api_key(
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

#[utoipa::path(
    patch,
    path = "/account/api-keys/{id}",
    tag = "auth",
    params(
        ("id" = String, Path, description = "API key ID to revoke"),
    ),
    responses(
        (status = 200, description = "API key revoked", body = MessageResponse),
        (status = 400, description = "Invalid key ID", body = ErrorBody),
        (status = 404, description = "Key not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn revoke_api_key(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(key_id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let key_uuid: uuid::Uuid = key_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid key ID",
            "validation_error",
        )
    })?;

    let result =
        sqlx::query("UPDATE api_keys SET active = false WHERE id = $1 AND account_id = $2")
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

#[utoipa::path(
    get,
    path = "/account/billing",
    tag = "auth",
    responses(
        (status = 200, description = "Billing information", body = BillingResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn get_billing(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<BillingResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let row = sqlx::query(
        "SELECT tier, stripe_customer_id, credits_balance, \
         auto_topup_enabled, auto_topup_amount, auto_topup_threshold, monthly_spend_limit \
         FROM accounts WHERE id = $1",
    )
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
        credits_balance: row.get("credits_balance"),
        auto_topup_enabled: row.get("auto_topup_enabled"),
        auto_topup_amount: row.get("auto_topup_amount"),
        auto_topup_threshold: row.get("auto_topup_threshold"),
        monthly_spend_limit: row.get("monthly_spend_limit"),
    }))
}

#[utoipa::path(
    patch,
    path = "/account/billing",
    tag = "auth",
    request_body = UpdateBillingRequest,
    responses(
        (status = 200, description = "Billing tier updated", body = MessageResponse),
        (status = 400, description = "Invalid tier", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_billing(
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
// Billing: top-up, auto top-up, spend limit, transactions
// ============================================================================

#[utoipa::path(
    post,
    path = "/account/billing/topup",
    tag = "auth",
    request_body = TopupRequest,
    responses(
        (status = 200, description = "Credits topped up", body = TopupResponse),
        (status = 400, description = "Invalid amount", body = ErrorBody),
        (status = 403, description = "Spend limit exceeded", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn topup_credits(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<TopupRequest>,
) -> Result<Json<TopupResponse>, ApiError> {
    if req.amount <= 0 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Amount must be positive",
            "validation_error",
        ));
    }

    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|e| {
            error!(user_id = %user.user_id, "topup: failed to get account_id: {e:?}");
            err(StatusCode::NOT_FOUND, "Account not found", "not_found")
        })?;

    // Check monthly spend limit
    check_spend_limit(&state.pool, account_id, req.amount).await?;

    let mut tx = state.pool.begin().await.map_err(|e| {
        error!(account_id = %account_id, "topup: failed to begin transaction: {e}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let new_balance: i64 = sqlx::query_scalar(
        "UPDATE accounts SET credits_balance = credits_balance + $1 WHERE id = $2 RETURNING credits_balance",
    )
    .bind(req.amount)
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!(account_id = %account_id, amount = req.amount, "topup: failed to update balance: {e}");
        err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update balance", "internal_error")
    })?;

    let tx_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO transactions (account_id, type, amount, balance_after, description) \
         VALUES ($1, 'manual_topup', $2, $3, 'Manual credit top-up') RETURNING id",
    )
    .bind(account_id)
    .bind(req.amount)
    .bind(new_balance)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!(account_id = %account_id, "topup: failed to insert transaction: {e}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to log transaction",
            "internal_error",
        )
    })?;

    tx.commit().await.map_err(|e| {
        error!(account_id = %account_id, "topup: failed to commit: {e}");
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    info!(account_id = %account_id, amount = req.amount, new_balance, "Manual credit top-up");

    Ok(Json(TopupResponse {
        credits_balance: new_balance,
        transaction_id: tx_id.to_string(),
        message: format!("Added {} credits", req.amount),
    }))
}

#[utoipa::path(
    patch,
    path = "/account/billing/auto-topup",
    tag = "auth",
    request_body = AutoTopupRequest,
    responses(
        (status = 200, description = "Auto top-up settings updated", body = MessageResponse),
        (status = 400, description = "Validation error", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_auto_topup(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<AutoTopupRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    if req.enabled {
        let amount = req.amount.unwrap_or(5000);
        let threshold = req.threshold.unwrap_or(500);
        if amount <= 0 || threshold < 0 {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "Amount must be positive and threshold non-negative",
                "validation_error",
            ));
        }
        sqlx::query(
            "UPDATE accounts SET auto_topup_enabled = true, auto_topup_amount = $1, auto_topup_threshold = $2 WHERE id = $3",
        )
        .bind(amount)
        .bind(threshold)
        .bind(account_id)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to update", "internal_error")
        })?;
    } else {
        sqlx::query("UPDATE accounts SET auto_topup_enabled = false WHERE id = $1")
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

    Ok(Json(MessageResponse {
        message: if req.enabled {
            "Auto top-up enabled".to_string()
        } else {
            "Auto top-up disabled".to_string()
        },
    }))
}

#[utoipa::path(
    patch,
    path = "/account/billing/spend-limit",
    tag = "auth",
    request_body = SpendLimitRequest,
    responses(
        (status = 200, description = "Spend limit updated", body = MessageResponse),
        (status = 400, description = "Invalid limit", body = ErrorBody),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn update_spend_limit(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<SpendLimitRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    if let Some(limit) = req.monthly_spend_limit {
        if limit <= 0 {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "Spend limit must be positive",
                "validation_error",
            ));
        }
    }

    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    sqlx::query("UPDATE accounts SET monthly_spend_limit = $1 WHERE id = $2")
        .bind(req.monthly_spend_limit)
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
        message: match req.monthly_spend_limit {
            Some(limit) => format!("Monthly spend limit set to {}", limit),
            None => "Monthly spend limit removed".to_string(),
        },
    }))
}

#[utoipa::path(
    get,
    path = "/account/billing/transactions",
    tag = "auth",
    params(
        ("limit" = Option<i64>, Query, description = "Maximum number of transactions to return (default 50, max 200)"),
        ("offset" = Option<i64>, Query, description = "Offset for pagination (default 0)"),
    ),
    responses(
        (status = 200, description = "List of transactions", body = TransactionsListResponse),
        (status = 404, description = "Account not found", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn list_transactions(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<TransactionsListResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let limit: i64 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(200);
    let offset: i64 = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE account_id = $1")
        .bind(account_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?;

    let rows = sqlx::query(
        "SELECT id, type, amount, balance_after, description, created_at \
         FROM transactions WHERE account_id = $1 \
         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(account_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let transactions: Vec<TransactionResponse> = rows
        .iter()
        .map(|r| TransactionResponse {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            tx_type: r.get("type"),
            amount: r.get("amount"),
            balance_after: r.get("balance_after"),
            description: r.get("description"),
            created_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        })
        .collect();

    Ok(Json(TransactionsListResponse {
        transactions,
        total,
    }))
}

/// Check if a top-up would exceed the monthly spend limit
async fn check_spend_limit(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
    amount: i64,
) -> Result<(), ApiError> {
    let row = sqlx::query("SELECT monthly_spend_limit FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            error!(account_id = %account_id, "check_spend_limit: failed to query account: {e}");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let limit: Option<i64> = row.get("monthly_spend_limit");
    if let Some(limit) = limit {
        // Sum all top-ups this calendar month
        let spent: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions \
             WHERE account_id = $1 \
             AND type IN ('manual_topup', 'auto_topup') \
             AND created_at >= date_trunc('month', now())",
        )
        .bind(account_id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            error!(account_id = %account_id, "check_spend_limit: failed to sum transactions: {e}");
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
                "internal_error",
            )
        })?;

        if spent + amount > limit {
            return Err(err(
                StatusCode::FORBIDDEN,
                "Monthly spend limit reached",
                "spend_limit_exceeded",
            ));
        }
    }

    Ok(())
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
        .route("/account/billing/topup", post(topup_credits))
        .route("/account/billing/auto-topup", patch(update_auto_topup))
        .route("/account/billing/spend-limit", patch(update_spend_limit))
        .route("/account/billing/transactions", get(list_transactions))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            super::validate_session,
        ))
        .with_state(state)
}
