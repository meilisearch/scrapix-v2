use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use tracing::info;

use super::{
    err, get_user_account_id, get_user_role, require_role, AcceptInviteRequest, ApiError,
    InviteMemberRequest, InviteResponse, MemberResponse, MessageResponse, UpdateMemberRoleRequest,
};
use crate::auth::{AuthState, AuthenticatedUser};

/// GET /account/members -- list all members of the current account
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

/// POST /account/members/invite -- invite a user by email
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
        mailer
            .queue_team_invite(
                &state.pool,
                req.email.trim(),
                &account_name.unwrap_or_else(|| "Scrapix".to_string()),
                &inviter_name,
                role,
                &raw_token,
            )
            .await;
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

/// PATCH /account/members/{user_id} -- change a member's role (owner only)
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

/// DELETE /account/members/{user_id} -- remove a member (owner, or self-remove)
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

/// GET /account/invites -- list pending invites for the current account
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

/// DELETE /account/invites/{id} -- revoke a pending invite
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

/// POST /auth/accept-invite -- accept an invite using a token (public, no auth required for the endpoint but user must be logged in)
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
