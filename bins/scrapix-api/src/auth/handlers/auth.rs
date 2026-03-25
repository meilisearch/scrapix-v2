use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    Json,
};
use axum_extra::extract::CookieJar;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tracing::info;

use super::{
    build_session_cookie, clear_session_cookie, err, AccountResponse, ApiError, ErrorBody,
    ForgotPasswordRequest, LoginRequest, MessageResponse, ResetPasswordRequest, SignupRequest,
    UserResponse, VerifyEmailQuery,
};
use crate::auth::{AuthState, AuthenticatedUser};
use scrapix_auth::{jwt, password};

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

    // Send verification email via the reliable queue
    if let Some(ref mailer) = state.email_client {
        let name = req.full_name.as_deref().unwrap_or("");
        mailer
            .queue_verification_email(&state.pool, &req.email, name, &verification_token)
            .await;
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
        mailer
            .queue_verification_email(&state.pool, &user.email, name, &token)
            .await;
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

    // Send password reset via the reliable queue
    if let Some(ref mailer) = state.email_client {
        mailer
            .queue_password_reset(&state.pool, &req.email, &raw_token)
            .await;
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
