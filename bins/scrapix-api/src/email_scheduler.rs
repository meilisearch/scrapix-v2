//! Postgres-backed delayed email queue with retry.
//!
//! Emails are inserted into `scheduled_emails` with a future `send_at` timestamp.
//! A background poller picks them up every 30 seconds and dispatches via the
//! [`EmailClient`]. Failed deliveries are retried with exponential backoff
//! (up to 5 attempts).

use sqlx::{PgPool, Row};
use tracing::{info, warn};

use crate::email::EmailClient;

/// Maximum number of delivery attempts before giving up.
const MAX_ATTEMPTS: i32 = 5;

// ============================================================================
// Schedule helpers (insert into the queue)
// ============================================================================

/// Schedule an email to be sent at `send_at`.
pub async fn schedule_email(
    pool: &PgPool,
    email_type: &str,
    recipient: &str,
    payload: serde_json::Value,
    send_at: chrono::DateTime<chrono::Utc>,
) {
    let result = sqlx::query(
        "INSERT INTO scheduled_emails (email_type, recipient, payload, send_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(email_type)
    .bind(recipient)
    .bind(&payload)
    .bind(send_at)
    .execute(pool)
    .await;

    match result {
        Ok(_) => info!(email_type, recipient, %send_at, "Email scheduled"),
        Err(e) => warn!(error = %e, email_type, recipient, "Failed to schedule email"),
    }
}

/// Schedule an email for immediate delivery (send_at = now).
pub async fn schedule_email_now(
    pool: &PgPool,
    email_type: &str,
    recipient: &str,
    payload: serde_json::Value,
) {
    schedule_email(pool, email_type, recipient, payload, chrono::Utc::now()).await;
}

// ============================================================================
// Background poller
// ============================================================================

pub fn spawn_email_scheduler(
    pool: PgPool,
    mailer: EmailClient,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = process_due_emails(&pool, &mailer).await {
                        warn!(error = %e, "Email scheduler tick failed");
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("Email scheduler shutting down");
                    break;
                }
            }
        }
    })
}

async fn process_due_emails(pool: &PgPool, mailer: &EmailClient) -> Result<(), sqlx::Error> {
    // Fetch due emails that haven't exceeded max attempts
    let rows = sqlx::query(
        "UPDATE scheduled_emails \
         SET attempts = attempts + 1 \
         WHERE id IN ( \
             SELECT id FROM scheduled_emails \
             WHERE sent = false \
               AND attempts < $1 \
               AND (next_attempt_at IS NULL OR next_attempt_at <= now()) \
               AND send_at <= now() \
             ORDER BY send_at \
             LIMIT 50 \
             FOR UPDATE SKIP LOCKED \
         ) \
         RETURNING id, email_type, recipient, payload, attempts",
    )
    .bind(MAX_ATTEMPTS)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    info!(
        count = rows.len(),
        "Email scheduler: dispatching due emails"
    );

    for row in &rows {
        let id: uuid::Uuid = row.get("id");
        let email_type: String = row.get("email_type");
        let recipient: String = row.get("recipient");
        let payload: serde_json::Value = row.get("payload");
        let attempts: i32 = row.get("attempts");

        match dispatch(mailer, &email_type, &recipient, &payload).await {
            Ok(()) => {
                // Mark as sent
                sqlx::query("UPDATE scheduled_emails SET sent = true WHERE id = $1")
                    .bind(id)
                    .execute(pool)
                    .await
                    .ok();
            }
            Err(reason) => {
                // Schedule retry with exponential backoff: 30s, 60s, 120s, 240s, 480s
                let backoff_secs = 30 * (1i64 << attempts.min(5));
                let next_attempt = chrono::Utc::now() + chrono::Duration::seconds(backoff_secs);

                warn!(
                    email_type,
                    recipient,
                    attempts,
                    next_attempt = %next_attempt,
                    error = %reason,
                    "Email dispatch failed, scheduling retry"
                );

                sqlx::query(
                    "UPDATE scheduled_emails SET next_attempt_at = $1, last_error = $2 WHERE id = $3",
                )
                .bind(next_attempt)
                .bind(&reason)
                .bind(id)
                .execute(pool)
                .await
                .ok();
            }
        }
    }

    Ok(())
}

/// Dispatch an email via `send_checked` (awaited, returns Result).
///
/// Builds the email payload using `build_*_payload` methods, then sends via
/// the Resend API with real error propagation for retry logic.
async fn dispatch(
    mailer: &EmailClient,
    email_type: &str,
    recipient: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let email_payload = match email_type {
        "welcome" => {
            let name = payload["name"].as_str().unwrap_or("");
            mailer.build_welcome_payload(recipient, name)
        }
        "verification" => {
            let name = payload["name"].as_str().unwrap_or("");
            let token = payload["token"].as_str().unwrap_or("");
            if token.is_empty() {
                return Err("Missing token".to_string());
            }
            mailer.build_verification_email_payload(recipient, name, token)
        }
        "password_reset" => {
            let token = payload["token"].as_str().unwrap_or("");
            if token.is_empty() {
                return Err("Missing token".to_string());
            }
            mailer.build_password_reset_payload(recipient, token)
        }
        "password_changed" => mailer.build_password_changed_payload(recipient),
        "payment_receipt" => {
            let credits = payload["credits"].as_i64().unwrap_or(0);
            let amount_cents = payload["amount_cents"].as_i64().unwrap_or(0);
            mailer.build_payment_receipt_payload(recipient, credits, amount_cents)
        }
        "auto_topup_receipt" => {
            let credits = payload["credits"].as_i64().unwrap_or(0);
            let amount_cents = payload["amount_cents"].as_i64().unwrap_or(0);
            let new_balance = payload["new_balance"].as_i64().unwrap_or(0);
            mailer.build_auto_topup_receipt_payload(recipient, credits, amount_cents, new_balance)
        }
        "auto_topup_failed" => {
            let reason = payload["reason"].as_str().unwrap_or("Unknown error");
            mailer.build_auto_topup_failed_payload(recipient, reason)
        }
        "job_completed" => {
            let job_id = payload["job_id"].as_str().unwrap_or("");
            let index_uid = payload["index_uid"].as_str().unwrap_or("");
            let pages_crawled = payload["pages_crawled"].as_u64().unwrap_or(0);
            let documents_indexed = payload["documents_indexed"].as_u64().unwrap_or(0);
            let duration_secs = payload["duration_secs"].as_u64().unwrap_or(0);
            mailer.build_job_completed_payload(
                recipient,
                job_id,
                index_uid,
                pages_crawled,
                documents_indexed,
                duration_secs,
            )
        }
        "job_failed" => {
            let job_id = payload["job_id"].as_str().unwrap_or("");
            let error_message = payload["error_message"].as_str().unwrap_or("Unknown error");
            let pages_crawled = payload["pages_crawled"].as_u64().unwrap_or(0);
            mailer.build_job_failed_payload(recipient, job_id, error_message, pages_crawled)
        }
        "team_invite" => {
            let account_name = payload["account_name"].as_str().unwrap_or("");
            let inviter_name = payload["inviter_name"].as_str().unwrap_or("");
            let role = payload["role"].as_str().unwrap_or("member");
            let token = payload["token"].as_str().unwrap_or("");
            if token.is_empty() {
                return Err("Missing token".to_string());
            }
            mailer.build_team_invite_payload(recipient, account_name, inviter_name, role, token)
        }
        "invite_accepted" => {
            let member_name = payload["member_name"].as_str().unwrap_or("");
            let account_name = payload["account_name"].as_str().unwrap_or("");
            let role = payload["role"].as_str().unwrap_or("member");
            mailer.build_invite_accepted_payload(recipient, member_name, account_name, role)
        }
        "member_removed" => {
            let account_name = payload["account_name"].as_str().unwrap_or("");
            let removed_by = payload["removed_by"].as_str().unwrap_or("");
            mailer.build_member_removed_payload(recipient, account_name, removed_by)
        }
        "low_balance" => {
            let balance = payload["balance"].as_i64().unwrap_or(0);
            mailer.build_low_balance_warning_payload(recipient, balance)
        }
        other => {
            warn!(
                email_type = other,
                recipient, "Unknown scheduled email type, skipping"
            );
            return Err(format!("Unknown email type: {other}"));
        }
    };

    mailer.send_checked(email_payload).await
}
