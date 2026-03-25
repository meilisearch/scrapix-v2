use axum::{
    http::StatusCode,
    middleware,
    routing::{delete, get, patch, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AuthState;

pub(crate) mod account;
pub(crate) mod api_keys;
pub(crate) mod auth;
pub(crate) mod billing;
pub(crate) mod team;

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SignupRequest {
    pub(super) email: String,
    pub(super) password: String,
    pub(super) full_name: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    pub(super) email: String,
    pub(super) password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct UserResponse {
    pub(super) id: String,
    pub(super) email: String,
    pub(super) full_name: Option<String>,
    pub(super) email_verified: bool,
    pub(super) notify_job_emails: bool,
    pub(super) account: Option<AccountResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AccountResponse {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) tier: String,
    pub(super) active: bool,
    pub(super) role: String,
    pub(super) credits_balance: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateMeRequest {
    pub(super) full_name: Option<String>,
    pub(super) notify_job_emails: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateAccountRequest {
    pub(super) name: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ApiKeyResponse {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) prefix: String,
    pub(super) active: bool,
    pub(super) last_used_at: Option<String>,
    pub(super) created_at: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateApiKeyRequest {
    pub(super) name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct CreatedApiKeyResponse {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) prefix: String,
    pub(super) key: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct BillingResponse {
    pub(super) tier: String,
    pub(super) stripe_customer_id: Option<String>,
    pub(super) credits_balance: i64,
    pub(super) auto_topup_enabled: bool,
    pub(super) auto_topup_amount: i64,
    pub(super) auto_topup_threshold: i64,
    pub(super) monthly_spend_limit: Option<i64>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateBillingRequest {
    pub(super) tier: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TopupRequest {
    pub(super) amount: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AutoTopupRequest {
    pub(super) enabled: bool,
    pub(super) amount: Option<i64>,
    pub(super) threshold: Option<i64>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SpendLimitRequest {
    pub(super) monthly_spend_limit: Option<i64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TransactionResponse {
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) tx_type: String,
    pub(super) amount: i64,
    pub(super) balance_after: i64,
    pub(super) description: Option<String>,
    pub(super) created_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TransactionsListResponse {
    pub(super) transactions: Vec<TransactionResponse>,
    pub(super) total: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TopupResponse {
    pub(super) credits_balance: i64,
    pub(super) transaction_id: String,
    pub(super) message: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct MessageResponse {
    pub(super) message: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ErrorBody {
    pub(super) error: String,
    pub(super) code: String,
}

pub(super) type ApiError = (StatusCode, Json<ErrorBody>);

#[derive(Deserialize, utoipa::ToSchema)]
pub struct VerifyEmailQuery {
    pub(super) token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ForgotPasswordRequest {
    pub(super) email: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ResetPasswordRequest {
    pub(super) token: String,
    pub(super) password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct AccountListItem {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) tier: String,
    pub(super) active: bool,
    pub(super) role: String,
    pub(super) credits_balance: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct MemberResponse {
    pub(super) user_id: String,
    pub(super) email: String,
    pub(super) full_name: Option<String>,
    pub(super) role: String,
    pub(super) joined_at: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct InviteMemberRequest {
    pub(super) email: String,
    pub(super) role: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateMemberRoleRequest {
    pub(super) role: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct InviteResponse {
    pub(super) id: String,
    pub(super) email: String,
    pub(super) role: String,
    pub(super) status: String,
    pub(super) invited_by: String,
    pub(super) expires_at: String,
    pub(super) created_at: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AcceptInviteRequest {
    pub(super) token: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct CreateAccountRequest {
    pub(super) name: String,
}

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

pub(super) fn clear_session_cookie() -> Cookie<'static> {
    Cookie::build(("scrapix_session", ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::ZERO)
        .build()
}

pub(super) fn err(status: StatusCode, msg: &str, code: &str) -> ApiError {
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
// Re-exports for openapi.rs (crate::auth::handlers::handler_name)
// ============================================================================

pub(crate) use account::{
    __path_get_account, __path_get_me, __path_update_account, __path_update_me,
};
pub(crate) use account::{
    create_account, get_account, get_me, list_my_accounts, update_account, update_me,
};
pub(crate) use api_keys::{__path_create_api_key, __path_list_api_keys, __path_revoke_api_key};
pub(crate) use api_keys::{create_api_key, list_api_keys, revoke_api_key};
pub(crate) use auth::{__path_login, __path_logout, __path_signup};
pub(crate) use auth::{
    forgot_password, login, logout, resend_verification, reset_password, signup, verify_email,
};
pub(crate) use billing::{
    __path_get_billing, __path_list_transactions, __path_topup_credits, __path_update_auto_topup,
    __path_update_billing, __path_update_spend_limit,
};
pub(crate) use billing::{
    get_billing, list_transactions, topup_credits, update_auto_topup, update_billing,
    update_spend_limit,
};
pub(crate) use team::{
    accept_invite, invite_member, list_invites, list_members, remove_member, revoke_invite,
    update_member_role,
};

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
