use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{delete, get, patch, post},
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
    email_verified: bool,
    notify_job_emails: bool,
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
    notify_job_emails: Option<bool>,
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

pub(crate) fn build_session_cookie(token: String) -> Cookie<'static> {
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

/// Get the user's account_id via account_members.
/// If `selected_account_id` is provided, verifies the user is a member of that account.
/// Otherwise falls back to the first account (backward compatible).
pub(crate) async fn get_user_account_id(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    selected_account_id: Option<uuid::Uuid>,
) -> Result<uuid::Uuid, StatusCode> {
    if let Some(account_id) = selected_account_id {
        // Verify user is a member of the requested account
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM account_members WHERE user_id = $1 AND account_id = $2)",
        )
        .bind(user_id)
        .bind(account_id)
        .fetch_one(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if exists {
            Ok(account_id)
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    } else {
        sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT account_id FROM account_members WHERE user_id = $1 LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
    }
}

/// Get the user's role in the given account.
pub(crate) async fn get_user_role(
    pool: &sqlx::PgPool,
    user_id: uuid::Uuid,
    account_id: uuid::Uuid,
) -> Result<String, StatusCode> {
    sqlx::query_scalar::<_, String>(
        "SELECT role FROM account_members WHERE user_id = $1 AND account_id = $2",
    )
    .bind(user_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)
}

/// Check that a role is in the allowed set. Returns Err(403) if not.
pub(crate) fn require_role(role: &str, allowed: &[&str]) -> Result<(), ApiError> {
    if allowed.contains(&role) {
        Ok(())
    } else {
        Err(err(
            StatusCode::FORBIDDEN,
            "Insufficient permissions",
            "forbidden",
        ))
    }
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

    // Generate email verification token (scoped to drop !Send ThreadRng before await)
    let verification_token = {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let token: String = (0..48)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
            .collect();
        token
    };

    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, full_name, email_verification_token) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(&req.email)
    .bind(&pw_hash)
    .bind(&req.full_name)
    .bind(&verification_token)
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

    // Auto-accept pending invites for this email
    let pending_invites = sqlx::query(
        "SELECT id, account_id, role FROM account_invites \
         WHERE email = $1 AND status = 'pending' AND expires_at > now()",
    )
    .bind(&req.email)
    .fetch_all(&state.pool)
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
        .execute(&state.pool)
        .await;

        let _ = sqlx::query("UPDATE account_invites SET status = 'accepted' WHERE id = $1")
            .bind(invite_id)
            .execute(&state.pool)
            .await;

        info!(user_id = %user_id, account_id = %inv_account_id, role = %inv_role, "Auto-accepted pending invite on signup");
    }

    // Send verification email (replaces welcome — welcome is sent on verification)
    if let Some(ref mailer) = state.email_client {
        let name = req.full_name.as_deref().unwrap_or("");
        mailer.send_verification_email(&req.email, name, &verification_token);
    }

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
            email_verified: false,
            notify_job_emails: true,
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
    let row = sqlx::query(
        "SELECT id, email, password_hash, full_name, email_verified, notify_job_emails \
         FROM users WHERE email = $1",
    )
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
    let email_verified: bool = row.get("email_verified");
    let notify_job_emails: bool = row.get("notify_job_emails");

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
            email_verified,
            notify_job_emails,
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
    let row = sqlx::query(
        "SELECT id, email, full_name, email_verified, notify_job_emails FROM users WHERE id = $1",
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
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "User not found", "not_found"))?;

    // If user has a selected_account_id, use that; otherwise default to first
    let account = if let Some(selected_id) = user.selected_account_id {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 AND m.account_id = $2",
        )
        .bind(user.user_id)
        .bind(selected_id)
        .fetch_optional(&state.pool)
        .await
    } else {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 LIMIT 1",
        )
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
    }
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
        email_verified: row.get("email_verified"),
        notify_job_emails: row.get("notify_job_emails"),
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
    if let Some(notify) = req.notify_job_emails {
        sqlx::query("UPDATE users SET notify_job_emails = $1 WHERE id = $2")
            .bind(notify)
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
    let row = if let Some(selected_id) = user.selected_account_id {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 AND m.account_id = $2",
        )
        .bind(user.user_id)
        .bind(selected_id)
        .fetch_optional(&state.pool)
        .await
    } else {
        sqlx::query(
            "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
             FROM account_members m JOIN accounts a ON a.id = m.account_id \
             WHERE m.user_id = $1 LIMIT 1",
        )
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
    }
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
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners can update account settings
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner"])?;

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
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
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

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners and admins can create API keys
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner", "admin"])?;

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
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners and admins can revoke API keys
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner", "admin"])?;

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
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
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

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Only owners can change billing tier
    let role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&role, &["owner"])?;

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

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
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
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
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

    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
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
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
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
            "SELECT COALESCE(SUM(amount), 0)::BIGINT FROM transactions \
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
// Email verification & Password reset
// ============================================================================

#[derive(Deserialize, utoipa::ToSchema)]
pub struct VerifyEmailQuery {
    token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ForgotPasswordRequest {
    email: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ResetPasswordRequest {
    token: String,
    password: String,
}

/// GET /auth/verify-email?token=xxx
///
/// Marks the user's email as verified. The token was sent via email on signup.
#[utoipa::path(
    get,
    path = "/auth/verify-email",
    tag = "auth",
    params(
        ("token" = String, Query, description = "Email verification token"),
    ),
    responses(
        (status = 200, description = "Email verified", body = MessageResponse),
        (status = 400, description = "Invalid or expired token", body = ErrorBody),
    )
)]
pub(crate) async fn verify_email(
    State(state): State<Arc<AuthState>>,
    Query(params): Query<VerifyEmailQuery>,
) -> Result<Json<MessageResponse>, ApiError> {
    let verified_user = sqlx::query(
        "UPDATE users SET email_verified = true, email_verification_token = NULL \
         WHERE email_verification_token = $1 AND email_verified = false \
         RETURNING email, full_name",
    )
    .bind(&params.token)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let verified_user = verified_user.ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid or expired verification token",
            "invalid_token",
        )
    })?;

    // Schedule welcome email ~2 minutes after verification (Postgres job queue)
    {
        let email: String = verified_user.get("email");
        let full_name: Option<String> = verified_user.get("full_name");
        let send_at = chrono::Utc::now() + chrono::Duration::seconds(120);
        let payload = serde_json::json!({ "name": full_name.as_deref().unwrap_or("") });
        crate::email_scheduler::schedule_email(&state.pool, "welcome", &email, payload, send_at)
            .await;
    }

    Ok(Json(MessageResponse {
        message: "Email verified successfully".to_string(),
    }))
}

/// POST /auth/resend-verification
///
/// Resends the verification email for the currently logged-in user.
#[utoipa::path(
    post,
    path = "/auth/resend-verification",
    tag = "auth",
    responses(
        (status = 200, description = "Verification email sent", body = MessageResponse),
        (status = 400, description = "Email already verified", body = ErrorBody),
    ),
    security(("api_key" = []))
)]
pub(crate) async fn resend_verification(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<MessageResponse>, ApiError> {
    // Check if already verified
    let row = sqlx::query("SELECT email_verified, full_name FROM users WHERE id = $1")
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

    let verified: bool = row.get("email_verified");
    if verified {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Email already verified",
            "already_verified",
        ));
    }

    let full_name: Option<String> = row.get("full_name");

    // Generate new token (scoped to drop !Send ThreadRng before await)
    let token = {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let t: String = (0..48)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
            .collect();
        t
    };

    sqlx::query("UPDATE users SET email_verification_token = $1 WHERE id = $2")
        .bind(&token)
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

    if let Some(ref mailer) = state.email_client {
        let name = full_name.as_deref().unwrap_or("");
        mailer.send_verification_email(&user.email, name, &token);
    }

    Ok(Json(MessageResponse {
        message: "Verification email sent".to_string(),
    }))
}

/// POST /auth/forgot-password
///
/// Sends a password reset email. Always returns 200 to prevent email enumeration.
#[utoipa::path(
    post,
    path = "/auth/forgot-password",
    tag = "auth",
    request_body = ForgotPasswordRequest,
    responses(
        (status = 200, description = "If the email exists, a reset link was sent", body = MessageResponse),
    )
)]
pub(crate) async fn forgot_password(
    State(state): State<Arc<AuthState>>,
    Json(req): Json<ForgotPasswordRequest>,
) -> Json<MessageResponse> {
    // Always return the same message to prevent email enumeration
    let generic_msg = Json(MessageResponse {
        message: "If an account with that email exists, we sent a password reset link.".to_string(),
    });

    // Look up user
    let user_row = sqlx::query("SELECT id FROM users WHERE email = $1")
        .bind(&req.email)
        .fetch_optional(&state.pool)
        .await;

    let user_id: uuid::Uuid = match user_row {
        Ok(Some(row)) => row.get("id"),
        _ => return generic_msg,
    };

    // Generate random token (scoped to drop !Send ThreadRng before await)
    let raw_token = {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let t: String = (0..48)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
            .collect();
        t
    };

    // Hash the token for storage (same pattern as API keys)
    let token_hash = {
        let mut hasher = Sha256::new();
        hasher.update(raw_token.as_bytes());
        hex::encode(hasher.finalize())
    };

    // Invalidate existing unused tokens for this user
    let _ = sqlx::query(
        "UPDATE password_reset_tokens SET used = true WHERE user_id = $1 AND used = false",
    )
    .bind(user_id)
    .execute(&state.pool)
    .await;

    // Insert new token (expires in 1 hour)
    let insert_result = sqlx::query(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at) \
         VALUES ($1, $2, now() + interval '1 hour')",
    )
    .bind(user_id)
    .bind(&token_hash)
    .execute(&state.pool)
    .await;

    if insert_result.is_err() {
        return generic_msg;
    }

    // Send email with the raw (unhashed) token
    if let Some(ref mailer) = state.email_client {
        mailer.send_password_reset(&req.email, &raw_token);
    }

    generic_msg
}

/// POST /auth/reset-password
///
/// Resets the user's password using a valid reset token.
#[utoipa::path(
    post,
    path = "/auth/reset-password",
    tag = "auth",
    request_body = ResetPasswordRequest,
    responses(
        (status = 200, description = "Password reset successfully", body = MessageResponse),
        (status = 400, description = "Invalid or expired token", body = ErrorBody),
    )
)]
pub(crate) async fn reset_password(
    State(state): State<Arc<AuthState>>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    if req.password.len() < 12 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Password must be at least 12 characters",
            "validation_error",
        ));
    }

    // Hash the provided token to look it up
    let token_hash = {
        let mut hasher = Sha256::new();
        hasher.update(req.token.as_bytes());
        hex::encode(hasher.finalize())
    };

    // Find the valid token
    let token_row = sqlx::query(
        "SELECT id, user_id FROM password_reset_tokens \
         WHERE token_hash = $1 AND used = false AND expires_at > now()",
    )
    .bind(&token_hash)
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
            StatusCode::BAD_REQUEST,
            "Invalid or expired reset token",
            "invalid_token",
        )
    })?;

    let token_id: uuid::Uuid = token_row.get("id");
    let user_id: uuid::Uuid = token_row.get("user_id");

    // Hash the new password
    let pw_hash = password::hash_password(&req.password).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to hash password",
            "internal_error",
        )
    })?;

    // In a transaction: mark token used + update password
    let mut tx = state.pool.begin().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    sqlx::query("UPDATE password_reset_tokens SET used = true WHERE id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to invalidate token",
                "internal_error",
            )
        })?;

    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&pw_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update password",
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

    // Send password changed confirmation email
    if let Some(ref mailer) = state.email_client {
        let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
        if let Some(email) = email {
            mailer.send_password_changed(&email);
        }
    }

    info!(user_id = %user_id, "Password reset successfully");

    Ok(Json(MessageResponse {
        message: "Password reset successfully. Please log in with your new password.".to_string(),
    }))
}

// ============================================================================
// Team management types
// ============================================================================

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AccountListItem {
    id: String,
    name: String,
    tier: String,
    active: bool,
    role: String,
    credits_balance: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct MemberResponse {
    user_id: String,
    email: String,
    full_name: Option<String>,
    role: String,
    joined_at: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct InviteMemberRequest {
    email: String,
    role: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateMemberRoleRequest {
    role: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct InviteResponse {
    id: String,
    email: String,
    role: String,
    status: String,
    invited_by: String,
    expires_at: String,
    created_at: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AcceptInviteRequest {
    token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateAccountRequest {
    name: String,
}

// ============================================================================
// Account switching: list all accounts + create new account
// ============================================================================

/// GET /auth/me/accounts — list all accounts the user belongs to
pub(crate) async fn list_my_accounts(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<AccountListItem>>, ApiError> {
    let rows = sqlx::query(
        "SELECT a.id, a.name, a.tier, a.active, a.credits_balance, m.role \
         FROM account_members m JOIN accounts a ON a.id = m.account_id \
         WHERE m.user_id = $1 ORDER BY m.joined_at ASC",
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let accounts: Vec<AccountListItem> = rows
        .iter()
        .map(|r| AccountListItem {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            name: r.get("name"),
            tier: r.get("tier"),
            active: r.get("active"),
            role: r.get("role"),
            credits_balance: r.get("credits_balance"),
        })
        .collect();

    Ok(Json(accounts))
}

/// POST /auth/me/accounts — create a new account (user becomes owner)
pub(crate) async fn create_account(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<AccountListItem>), ApiError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Account name is required",
            "validation_error",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    let account_id: uuid::Uuid =
        sqlx::query_scalar("INSERT INTO accounts (name) VALUES ($1) RETURNING id")
            .bind(&name)
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
        .bind(user.user_id)
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

    // Log the initial credit deposit
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

    info!(user_id = %user.user_id, account_id = %account_id, name = %name, "New account created");

    Ok((
        StatusCode::CREATED,
        Json(AccountListItem {
            id: account_id.to_string(),
            name,
            tier: "free".to_string(),
            active: true,
            role: "owner".to_string(),
            credits_balance: 100,
        }),
    ))
}

// ============================================================================
// Team member management
// ============================================================================

/// GET /account/members — list all members of the current account
pub(crate) async fn list_members(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<MemberResponse>>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let rows = sqlx::query(
        "SELECT u.id, u.email, u.full_name, m.role, m.joined_at \
         FROM account_members m JOIN users u ON u.id = m.user_id \
         WHERE m.account_id = $1 ORDER BY m.joined_at ASC",
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

    let members: Vec<MemberResponse> = rows
        .iter()
        .map(|r| MemberResponse {
            user_id: r.get::<uuid::Uuid, _>("id").to_string(),
            email: r.get("email"),
            full_name: r.get("full_name"),
            role: r.get("role"),
            joined_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("joined_at")
                .to_rfc3339(),
        })
        .collect();

    Ok(Json(members))
}

/// POST /account/members/invite — invite a user by email
pub(crate) async fn invite_member(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<InviteMemberRequest>,
) -> Result<Json<InviteResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    // Check caller is owner or admin
    let caller_role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&caller_role, &["owner", "admin"])?;

    let role = req.role.as_deref().unwrap_or("member");
    if !["admin", "member", "viewer"].contains(&role) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Invalid role. Must be admin, member, or viewer",
            "validation_error",
        ));
    }

    // Admins cannot invite admins or owners
    if caller_role == "admin" && role == "admin" {
        return Err(err(
            StatusCode::FORBIDDEN,
            "Admins cannot invite other admins",
            "forbidden",
        ));
    }

    if req.email.trim().is_empty() || !req.email.contains('@') {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Valid email is required",
            "validation_error",
        ));
    }

    // Check if user is already a member
    let already_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_members m JOIN users u ON u.id = m.user_id \
         WHERE m.account_id = $1 AND u.email = $2)",
    )
    .bind(account_id)
    .bind(req.email.trim())
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    if already_member {
        return Err(err(
            StatusCode::CONFLICT,
            "User is already a member of this account",
            "already_member",
        ));
    }

    // Generate invite token (scoped to drop !Send ThreadRng before await)
    let (raw_token, token_hash) = {
        use rand::Rng;
        let chars: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let raw: String = (0..48)
            .map(|_| chars[rng.gen_range(0..chars.len())] as char)
            .collect();
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let hash = hex::encode(hasher.finalize());
        (raw, hash)
    };

    let row = sqlx::query(
        "INSERT INTO account_invites (account_id, email, role, invited_by, token_hash) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (account_id, email) WHERE status = 'pending' \
         DO UPDATE SET role = EXCLUDED.role, token_hash = EXCLUDED.token_hash, \
             expires_at = now() + interval '7 days', invited_by = EXCLUDED.invited_by \
         RETURNING id, email, role, status, expires_at, created_at",
    )
    .bind(account_id)
    .bind(req.email.trim())
    .bind(role)
    .bind(user.user_id)
    .bind(&token_hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create invite",
            "internal_error",
        )
    })?;

    // Send invite email
    if let Some(ref mailer) = state.email_client {
        // Get account name for the email
        let account_name: Option<String> =
            sqlx::query_scalar("SELECT name FROM accounts WHERE id = $1")
                .bind(account_id)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
        let inviter_name = user.email.clone();
        mailer.send_team_invite(
            req.email.trim(),
            &account_name.unwrap_or_else(|| "Scrapix".to_string()),
            &inviter_name,
            role,
            &raw_token,
        );
    }

    info!(account_id = %account_id, invited_email = %req.email, role = %role, "Team invite sent");

    Ok(Json(InviteResponse {
        id: row.get::<uuid::Uuid, _>("id").to_string(),
        email: row.get("email"),
        role: row.get("role"),
        status: row.get("status"),
        invited_by: user.user_id.to_string(),
        expires_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("expires_at")
            .to_rfc3339(),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
    }))
}

/// PATCH /account/members/{user_id} — change a member's role (owner only)
pub(crate) async fn update_member_role(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(member_user_id): Path<String>,
    Json(req): Json<UpdateMemberRoleRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let caller_role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&caller_role, &["owner"])?;

    let target_user_id: uuid::Uuid = member_user_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid user ID",
            "validation_error",
        )
    })?;

    if !["owner", "admin", "member", "viewer"].contains(&req.role.as_str()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Invalid role",
            "validation_error",
        ));
    }

    // Don't allow changing own role
    if target_user_id == user.user_id {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Cannot change your own role",
            "validation_error",
        ));
    }

    let result =
        sqlx::query("UPDATE account_members SET role = $1 WHERE user_id = $2 AND account_id = $3")
            .bind(&req.role)
            .bind(target_user_id)
            .bind(account_id)
            .execute(&state.pool)
            .await
            .map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update role",
                    "internal_error",
                )
            })?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Member not found", "not_found"));
    }

    info!(account_id = %account_id, target_user_id = %target_user_id, new_role = %req.role, "Member role updated");

    Ok(Json(MessageResponse {
        message: format!("Role updated to {}", req.role),
    }))
}

/// DELETE /account/members/{user_id} — remove a member (owner, or self-remove)
pub(crate) async fn remove_member(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(member_user_id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let target_user_id: uuid::Uuid = member_user_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid user ID",
            "validation_error",
        )
    })?;

    // Self-remove is always allowed (except for owners)
    let is_self = target_user_id == user.user_id;

    if is_self {
        let my_role = get_user_role(&state.pool, user.user_id, account_id)
            .await
            .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
        if my_role == "owner" {
            // Check if there's another owner
            let owner_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM account_members WHERE account_id = $1 AND role = 'owner'",
            )
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

            if owner_count <= 1 {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "Cannot leave: you are the only owner. Transfer ownership first.",
                    "last_owner",
                ));
            }
        }
    } else {
        // Only owner can remove others
        let caller_role = get_user_role(&state.pool, user.user_id, account_id)
            .await
            .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
        require_role(&caller_role, &["owner"])?;
    }

    let result = sqlx::query("DELETE FROM account_members WHERE user_id = $1 AND account_id = $2")
        .bind(target_user_id)
        .bind(account_id)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to remove member",
                "internal_error",
            )
        })?;

    if result.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "Member not found", "not_found"));
    }

    info!(account_id = %account_id, removed_user_id = %target_user_id, "Member removed");

    // Notify the removed member (only if removed by someone else, not self-removal)
    if !is_self {
        if let Some(ref mailer) = state.email_client {
            let pool = state.pool.clone();
            let mailer = mailer.clone();
            let remover_email = user.email.clone();
            tokio::spawn(async move {
                let removed_email = crate::email::get_user_email(&pool, target_user_id).await;
                let account_name = crate::email::get_account_name(&pool, account_id).await;

                if let Some(removed_email) = removed_email {
                    mailer.send_member_removed(
                        &removed_email,
                        &account_name.unwrap_or_else(|| "Scrapix".to_string()),
                        &remover_email,
                    );
                }
            });
        }
    }

    Ok(Json(MessageResponse {
        message: "Member removed".to_string(),
    }))
}

// ============================================================================
// Invite management
// ============================================================================

/// GET /account/invites — list pending invites for the current account
pub(crate) async fn list_invites(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<InviteResponse>>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let caller_role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&caller_role, &["owner", "admin"])?;

    let rows = sqlx::query(
        "SELECT i.id, i.email, i.role, i.status, i.invited_by, i.expires_at, i.created_at \
         FROM account_invites i WHERE i.account_id = $1 AND i.status = 'pending' \
         AND i.expires_at > now() ORDER BY i.created_at DESC",
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

    let invites: Vec<InviteResponse> = rows
        .iter()
        .map(|r| InviteResponse {
            id: r.get::<uuid::Uuid, _>("id").to_string(),
            email: r.get("email"),
            role: r.get("role"),
            status: r.get("status"),
            invited_by: r.get::<uuid::Uuid, _>("invited_by").to_string(),
            expires_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("expires_at")
                .to_rfc3339(),
            created_at: r
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        })
        .collect();

    Ok(Json(invites))
}

/// DELETE /account/invites/{id} — revoke a pending invite
pub(crate) async fn revoke_invite(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(invite_id): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let account_id = get_user_account_id(&state.pool, user.user_id, user.selected_account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;

    let caller_role = get_user_role(&state.pool, user.user_id, account_id)
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "Account not found", "not_found"))?;
    require_role(&caller_role, &["owner", "admin"])?;

    let invite_uuid: uuid::Uuid = invite_id.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "Invalid invite ID",
            "validation_error",
        )
    })?;

    let result = sqlx::query(
        "UPDATE account_invites SET status = 'revoked' \
         WHERE id = $1 AND account_id = $2 AND status = 'pending'",
    )
    .bind(invite_uuid)
    .bind(account_id)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to revoke invite",
            "internal_error",
        )
    })?;

    if result.rows_affected() == 0 {
        return Err(err(
            StatusCode::NOT_FOUND,
            "Invite not found or already processed",
            "not_found",
        ));
    }

    Ok(Json(MessageResponse {
        message: "Invite revoked".to_string(),
    }))
}

/// POST /auth/accept-invite — accept an invite using a token (public, no auth required for the endpoint but user must be logged in)
pub(crate) async fn accept_invite(
    State(state): State<Arc<AuthState>>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(req): Json<AcceptInviteRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let token_hash = {
        let mut hasher = Sha256::new();
        hasher.update(req.token.as_bytes());
        hex::encode(hasher.finalize())
    };

    // Find the pending invite
    let invite_row = sqlx::query(
        "SELECT id, account_id, email, role FROM account_invites \
         WHERE token_hash = $1 AND status = 'pending' AND expires_at > now()",
    )
    .bind(&token_hash)
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
            StatusCode::BAD_REQUEST,
            "Invalid or expired invite token",
            "invalid_token",
        )
    })?;

    let invite_id: uuid::Uuid = invite_row.get("id");
    let account_id: uuid::Uuid = invite_row.get("account_id");
    let invite_email: String = invite_row.get("email");
    let role: String = invite_row.get("role");

    // Verify the logged-in user's email matches the invite
    if user.email.to_lowercase() != invite_email.to_lowercase() {
        return Err(err(
            StatusCode::FORBIDDEN,
            "This invite was sent to a different email address",
            "email_mismatch",
        ));
    }

    // Check if already a member
    let already_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_members WHERE user_id = $1 AND account_id = $2)",
    )
    .bind(user.user_id)
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

    if already_member {
        // Mark invite as accepted even if already a member
        let _ = sqlx::query("UPDATE account_invites SET status = 'accepted' WHERE id = $1")
            .bind(invite_id)
            .execute(&state.pool)
            .await;

        return Ok(Json(MessageResponse {
            message: "You are already a member of this account".to_string(),
        }));
    }

    // In a transaction: add member + mark invite accepted
    let mut tx = state.pool.begin().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
            "internal_error",
        )
    })?;

    sqlx::query("INSERT INTO account_members (user_id, account_id, role) VALUES ($1, $2, $3)")
        .bind(user.user_id)
        .bind(account_id)
        .bind(&role)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to add member",
                "internal_error",
            )
        })?;

    sqlx::query("UPDATE account_invites SET status = 'accepted' WHERE id = $1")
        .bind(invite_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update invite",
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

    info!(user_id = %user.user_id, account_id = %account_id, role = %role, "User accepted team invite");

    // Notify the inviter that the invite was accepted
    if let Some(ref mailer) = state.email_client {
        let pool = state.pool.clone();
        let mailer = mailer.clone();
        let member_name = user.email.clone();
        let role = role.clone();
        tokio::spawn(async move {
            // Get the inviter's user_id from the invite
            let inviter_id: Option<uuid::Uuid> =
                sqlx::query_scalar("SELECT invited_by FROM account_invites WHERE id = $1")
                    .bind(invite_id)
                    .fetch_optional(&pool)
                    .await
                    .ok()
                    .flatten();

            if let Some(inviter_id) = inviter_id {
                let inviter_email = crate::email::get_user_email(&pool, inviter_id).await;
                let account_name = crate::email::get_account_name(&pool, account_id).await;

                if let Some(inviter_email) = inviter_email {
                    mailer.send_invite_accepted(
                        &inviter_email,
                        &member_name,
                        &account_name.unwrap_or_else(|| "Scrapix".to_string()),
                        &role,
                    );
                }
            }
        });
    }

    Ok(Json(MessageResponse {
        message: format!("You have joined the account as {role}"),
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
        .route("/auth/verify-email", get(verify_email))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
        .with_state(state)
}

/// Routes protected by JWT session cookie
pub fn session_routes(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/auth/me", get(get_me).patch(update_me))
        .route(
            "/auth/me/accounts",
            get(list_my_accounts).post(create_account),
        )
        .route("/auth/accept-invite", post(accept_invite))
        .route("/auth/resend-verification", post(resend_verification))
        .route("/account", get(get_account).patch(update_account))
        .route("/account/members", get(list_members))
        .route("/account/members/invite", post(invite_member))
        .route(
            "/account/members/{user_id}",
            patch(update_member_role).delete(remove_member),
        )
        .route("/account/invites", get(list_invites))
        .route("/account/invites/{id}", delete(revoke_invite))
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
