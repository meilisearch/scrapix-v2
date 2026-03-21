//! Transactional email sending via Resend.
//!
//! All emails are sent asynchronously (fire-and-forget via `tokio::spawn`)
//! so they never block API request handling. Failures are logged but do not
//! propagate to callers.
//!
//! Templates use a dark theme matching the Scrapix console design:
//! - Background: #0a0a0f (near-black)
//! - Card: #18181b (zinc-900)
//! - Text: #e4e4e7 (zinc-200)
//! - Muted: #a1a1aa (zinc-400)
//! - Accent: #818cf8 (indigo-400)
//! - Borders: rgba(255,255,255,0.06)

use reqwest::Client;
use serde::Serialize;
use tracing::{info, warn};

const RESEND_API_URL: &str = "https://api.resend.com/emails";
const FROM_ADDRESS: &str = "Scrapix <noreply@scrapix.meilisearch.com>";
const LOGO_URL: &str = "https://scrapix.meilisearch.com/logotype_dark@2x.png";
const CONSOLE_URL: &str = "https://scrapix.meilisearch.com";

// ============================================================================
// Client
// ============================================================================

/// Resend email client. Clone-friendly (wraps an `Arc`-backed reqwest client).
#[derive(Clone)]
pub struct EmailClient {
    http: Client,
    api_key: String,
}

impl EmailClient {
    pub fn new(api_key: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
        }
    }
}

// ============================================================================
// Owned payload that is Send + 'static
// ============================================================================

#[derive(Serialize)]
struct OwnedSendEmailRequest {
    from: String,
    to: Vec<String>,
    subject: String,
    html: String,
}

impl EmailClient {
    /// Fire-and-forget with owned data (no lifetime issues).
    fn send_owned(&self, payload: OwnedSendEmailRequest) {
        let client = self.clone();
        tokio::spawn(async move {
            let resp = client
                .http
                .post(RESEND_API_URL)
                .bearer_auth(&client.api_key)
                .json(&payload)
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    info!(to = %payload.to.join(", "), subject = %payload.subject, "Email sent");
                }
                Ok(r) => {
                    let status = r.status();
                    let body = r.text().await.unwrap_or_default();
                    warn!(status = %status, body = %body, "Resend API error");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to send email via Resend");
                }
            }
        });
    }
}

// ============================================================================
// Template helpers
// ============================================================================

/// Wraps content in the branded dark email shell (logo, card, footer).
fn wrap(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta name="color-scheme" content="dark">
  <meta name="supported-color-schemes" content="dark">
  <title>{title}</title>
</head>
<body style="margin: 0; padding: 0; background-color: #0a0a0f; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; -webkit-font-smoothing: antialiased;">
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background-color: #0a0a0f;">
    <tr>
      <td align="center" style="padding: 40px 16px;">
        <table role="presentation" width="560" cellpadding="0" cellspacing="0" style="max-width: 560px; width: 100%;">
          <!-- Logo -->
          <tr>
            <td style="padding-bottom: 32px;">
              <a href="{CONSOLE_URL}" style="text-decoration: none;">
                <img src="{LOGO_URL}" alt="Scrapix" width="120" height="35" style="display: block; border: 0; height: 35px; width: auto;" />
              </a>
            </td>
          </tr>
          <!-- Card -->
          <tr>
            <td style="background-color: #18181b; border: 1px solid rgba(255,255,255,0.06); border-radius: 16px; padding: 40px 32px;">
              {body}
            </td>
          </tr>
          <!-- Footer -->
          <tr>
            <td style="padding-top: 32px; text-align: center;">
              <p style="margin: 0 0 8px; color: #52525b; font-size: 12px; line-height: 1.5;">
                <a href="{CONSOLE_URL}" style="color: #52525b; text-decoration: underline;">Console</a>
                &nbsp;&middot;&nbsp;
                <a href="https://docs.scrapix.meilisearch.com" style="color: #52525b; text-decoration: underline;">Docs</a>
                &nbsp;&middot;&nbsp;
                <a href="{CONSOLE_URL}/settings" style="color: #52525b; text-decoration: underline;">Settings</a>
              </p>
              <p style="margin: 0; color: #3f3f46; font-size: 11px;">Scrapix by Meilisearch</p>
            </td>
          </tr>
        </table>
      </td>
    </tr>
  </table>
</body>
</html>"#
    )
}

fn heading(text: &str) -> String {
    format!(
        r#"<h1 style="margin: 0 0 16px; font-size: 22px; font-weight: 700; color: #fafafa; line-height: 1.3;">{text}</h1>"#
    )
}

fn paragraph(text: &str) -> String {
    format!(
        r#"<p style="margin: 0 0 16px; font-size: 15px; line-height: 1.6; color: #a1a1aa;">{text}</p>"#
    )
}

fn button(url: &str, label: &str) -> String {
    format!(
        r#"<table role="presentation" cellpadding="0" cellspacing="0" style="margin: 24px 0;">
  <tr>
    <td style="background-color: #fff; border-radius: 10px;">
      <a href="{url}" style="display: inline-block; padding: 12px 28px; font-size: 14px; font-weight: 600; color: #09090b; text-decoration: none; border-radius: 10px;">{label}</a>
    </td>
  </tr>
</table>"#
    )
}

fn kv_row(label: &str, value: &str, is_last: bool) -> String {
    let border = if is_last {
        ""
    } else {
        "border-bottom: 1px solid rgba(255,255,255,0.06);"
    };
    format!(
        r#"<tr style="{border}">
  <td style="padding: 12px 0; font-size: 14px; color: #71717a;">{label}</td>
  <td style="padding: 12px 0; text-align: right; font-size: 14px; font-weight: 600; color: #e4e4e7;">{value}</td>
</tr>"#
    )
}

fn kv_row_mono(label: &str, value: &str, is_last: bool) -> String {
    let border = if is_last {
        ""
    } else {
        "border-bottom: 1px solid rgba(255,255,255,0.06);"
    };
    format!(
        r#"<tr style="{border}">
  <td style="padding: 12px 0; font-size: 14px; color: #71717a;">{label}</td>
  <td style="padding: 12px 0; text-align: right; font-size: 13px; font-family: 'SF Mono', SFMono-Regular, Consolas, 'Liberation Mono', Menlo, monospace; color: #a1a1aa;">{value}</td>
</tr>"#
    )
}

fn table_start() -> &'static str {
    r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin: 20px 0; border-collapse: collapse;">"#
}

fn table_end() -> &'static str {
    "</table>"
}

fn muted(text: &str) -> String {
    format!(
        r#"<p style="margin: 16px 0 0; font-size: 12px; line-height: 1.5; color: #52525b;">{text}</p>"#
    )
}

fn link(url: &str, text: &str) -> String {
    format!(r#"<a href="{url}" style="color: #818cf8; text-decoration: underline;">{text}</a>"#)
}

fn alert_box(text: &str, style: AlertStyle) -> String {
    let (bg, border, color) = match style {
        AlertStyle::Error => ("#2a1215", "rgba(248,113,113,0.2)", "#fca5a5"),
        AlertStyle::Warning => ("#2a2012", "rgba(251,191,36,0.2)", "#fcd34d"),
    };
    format!(
        r#"<div style="background: {bg}; border: 1px solid {border}; border-radius: 10px; padding: 16px; margin: 16px 0;">
  <p style="margin: 0; font-size: 14px; line-height: 1.5; color: {color};">{text}</p>
</div>"#
    )
}

enum AlertStyle {
    Error,
    Warning,
}

// ============================================================================
// Public send helpers — one per email type
// ============================================================================

impl EmailClient {
    // ------------------------------------------------------------------
    // 1. Welcome email (sent after email verification)
    // ------------------------------------------------------------------
    pub fn send_welcome(&self, to_email: &str, name: &str) {
        let name = if name.is_empty() { "there" } else { name };
        let welcome_heading = heading(&format!("Welcome, {name}!"));
        let console_link = link(&format!("{CONSOLE_URL}/settings/api-keys"), "console");
        let create_key_text = format!(
            "<strong style=\"color: #e4e4e7;\">Create an API key</strong> in the {console_link}"
        );
        let docs_link = link(
            "https://docs.scrapix.meilisearch.com",
            "docs.scrapix.meilisearch.com",
        );
        let docs_text = muted(&format!("Read the docs at {docs_link}"));
        let body = format!(
            "{welcome_heading}\
             {intro}\
             <p style=\"margin: 0 0 20px; font-size: 15px; line-height: 1.6; color: #e4e4e7;\">We've added <strong style=\"color: #fafafa;\">100 free credits</strong> to your account.</p>\
             {what_next}\
             <table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin: 0 0 16px;\">\
               <tr><td style=\"padding: 6px 0; font-size: 14px; color: #a1a1aa;\">\
                 <span style=\"color: #818cf8; font-weight: 600;\">1.</span>&nbsp; {create_key_text}\
               </td></tr>\
               <tr><td style=\"padding: 6px 0; font-size: 14px; color: #a1a1aa;\">\
                 <span style=\"color: #818cf8; font-weight: 600;\">2.</span>&nbsp; <strong style=\"color: #e4e4e7;\">Scrape a page</strong> with <code style=\"background: #27272a; padding: 2px 6px; border-radius: 4px; font-size: 13px; color: #818cf8;\">POST /scrape</code>\
               </td></tr>\
               <tr><td style=\"padding: 6px 0; font-size: 14px; color: #a1a1aa;\">\
                 <span style=\"color: #818cf8; font-weight: 600;\">3.</span>&nbsp; <strong style=\"color: #e4e4e7;\">Start a crawl</strong> with <code style=\"background: #27272a; padding: 2px 6px; border-radius: 4px; font-size: 13px; color: #818cf8;\">POST /crawl</code>\
               </td></tr>\
             </table>\
             {btn}\
             {docs_text}",
            intro = paragraph("Your Scrapix account is ready."),
            what_next = paragraph("Here's how to get started:"),
            btn = button(CONSOLE_URL, "Open Console"),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: "Welcome to Scrapix — your 100 free credits are ready".to_string(),
            html: wrap("Welcome to Scrapix", &body),
        });
    }

    // ------------------------------------------------------------------
    // 2. Payment receipt (Stripe payment succeeded)
    // ------------------------------------------------------------------
    pub fn send_payment_receipt(&self, to_email: &str, credits: i64, amount_cents: i64) {
        let dollars = format!("${:.2}", amount_cents as f64 / 100.0);
        let body = format!(
            "{heading}\
             {intro}\
             {ts}\
               {r1}\
               {r2}\
             {te}\
             {link}\
             {warn}",
            heading = heading("Payment Received"),
            intro = paragraph(&format!(
                "Your payment of <strong style=\"color: #fafafa;\">{dollars}</strong> has been processed."
            )),
            ts = table_start(),
            r1 = kv_row("Credits purchased", &credits.to_string(), false),
            r2 = kv_row("Amount charged", &dollars, true),
            te = table_end(),
            link = muted(&format!(
                "View billing at {}",
                link(&format!("{CONSOLE_URL}/settings/billing"), "scrapix.meilisearch.com/settings/billing")
            )),
            warn = muted("If you did not make this purchase, contact us at support@meilisearch.com."),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: format!("Payment receipt — {credits} credits"),
            html: wrap("Payment Received", &body),
        });
    }

    // ------------------------------------------------------------------
    // 3. Auto-topup success
    // ------------------------------------------------------------------
    pub fn send_auto_topup_receipt(
        &self,
        to_email: &str,
        credits: i64,
        amount_cents: i64,
        new_balance: i64,
    ) {
        let dollars = format!("${:.2}", amount_cents as f64 / 100.0);
        let body = format!(
            "{heading}\
             {intro}\
             {ts}\
               {r1}\
               {r2}\
               {r3}\
             {te}\
             {link}",
            heading = heading("Auto Top-Up Processed"),
            intro = paragraph("Your balance dropped below your threshold, so we automatically topped up your account."),
            ts = table_start(),
            r1 = kv_row("Credits added", &credits.to_string(), false),
            r2 = kv_row("Amount charged", &dollars, false),
            r3 = kv_row("New balance", &format!("{new_balance} credits"), true),
            te = table_end(),
            link = muted(&format!(
                "Manage auto top-up at {}",
                link(&format!("{CONSOLE_URL}/settings/billing"), "billing settings")
            )),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: format!("Auto top-up — {credits} credits added"),
            html: wrap("Auto Top-Up", &body),
        });
    }

    // ------------------------------------------------------------------
    // 4. Auto-topup failure
    // ------------------------------------------------------------------
    pub fn send_auto_topup_failed(&self, to_email: &str, reason: &str) {
        let reason = html_escape(reason);
        let body = format!(
            "{heading}\
             {intro}\
             {alert}\
             {steps}\
             {btn}",
            heading = heading("Auto Top-Up Failed"),
            intro = paragraph(
                "We tried to automatically top up your credits, but the charge was declined."
            ),
            alert = alert_box(
                &format!("<strong>Reason:</strong> {reason}"),
                AlertStyle::Error
            ),
            steps = paragraph(&format!(
                "Your crawl jobs may be paused until you have sufficient credits. \
                 Please check your {} or manually purchase credits.",
                link(&format!("{CONSOLE_URL}/settings/billing"), "payment method")
            )),
            btn = button(
                &format!("{CONSOLE_URL}/settings/billing"),
                "Update Payment Method"
            ),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: "Action required — auto top-up failed".to_string(),
            html: wrap("Auto Top-Up Failed", &body),
        });
    }

    // ------------------------------------------------------------------
    // 5. Job completed
    // ------------------------------------------------------------------
    pub fn send_job_completed(
        &self,
        to_email: &str,
        job_id: &str,
        index_uid: &str,
        pages_crawled: u64,
        documents_indexed: u64,
        duration_secs: u64,
    ) {
        let duration = format_duration(duration_secs);
        let body = format!(
            "{heading}\
             {intro}\
             {ts}\
               {r1}\
               {r2}\
               {r3}\
               {r4}\
               {r5}\
             {te}\
             {btn}",
            heading = heading("Crawl Complete"),
            intro = paragraph("Your crawl job has finished successfully."),
            ts = table_start(),
            r1 = kv_row_mono("Job ID", job_id, false),
            r2 = kv_row("Index", index_uid, false),
            r3 = kv_row("Pages crawled", &pages_crawled.to_string(), false),
            r4 = kv_row("Documents indexed", &documents_indexed.to_string(), false),
            r5 = kv_row("Duration", &duration, true),
            te = table_end(),
            btn = button(&format!("{CONSOLE_URL}/jobs/{job_id}"), "View Results"),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: format!(
                "Crawl complete — {documents_indexed} documents indexed in \"{index_uid}\""
            ),
            html: wrap("Crawl Complete", &body),
        });
    }

    // ------------------------------------------------------------------
    // 6. Job failed
    // ------------------------------------------------------------------
    pub fn send_job_failed(
        &self,
        to_email: &str,
        job_id: &str,
        error_message: &str,
        pages_crawled: u64,
    ) {
        let error_message = html_escape(error_message);
        let body = format!(
            "{heading}\
             {intro}\
             <div style=\"background: #27272a; border: 1px solid rgba(255,255,255,0.06); border-radius: 10px; padding: 16px; margin: 16px 0;\">\
               <p style=\"margin: 0; font-family: 'SF Mono', SFMono-Regular, Consolas, monospace; font-size: 13px; line-height: 1.5; color: #fca5a5; word-break: break-all;\">{error_message}</p>\
             </div>\
             {ts}\
               {r1}\
               {r2}\
             {te}\
             {btn}",
            heading = heading("Crawl Failed"),
            intro = paragraph("Your crawl job encountered an error and could not complete."),
            ts = table_start(),
            r1 = kv_row_mono("Job ID", job_id, false),
            r2 = kv_row("Pages before failure", &pages_crawled.to_string(), true),
            te = table_end(),
            btn = button(&format!("{CONSOLE_URL}/jobs/{job_id}"), "View Details"),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: format!("Crawl job failed — {job_id}"),
            html: wrap("Crawl Failed", &body),
        });
    }

    // ------------------------------------------------------------------
    // 7. Email verification
    // ------------------------------------------------------------------
    pub fn send_verification_email(&self, to_email: &str, name: &str, token: &str) {
        let name = if name.is_empty() { "there" } else { name };
        let verify_url = format!("{CONSOLE_URL}/auth/verify-email?token={token}");
        let body = format!(
            "{heading}\
             {intro}\
             {btn}\
             {fallback}\
             {ignore}",
            heading = heading(&format!("Hey {name}, verify your email")),
            intro = paragraph("Please verify your email address to activate your Scrapix account and start crawling."),
            btn = button(&verify_url, "Verify Email Address"),
            fallback = muted(&format!(
                "Or copy this link: <span style=\"color: #818cf8; word-break: break-all;\">{verify_url}</span>"
            )),
            ignore = muted("If you didn't create a Scrapix account, you can safely ignore this email."),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: "Verify your email address — Scrapix".to_string(),
            html: wrap("Verify Email", &body),
        });
    }

    // ------------------------------------------------------------------
    // 8. Password reset
    // ------------------------------------------------------------------
    pub fn send_password_reset(&self, to_email: &str, token: &str) {
        let reset_url = format!("{CONSOLE_URL}/auth/reset-password?token={token}");
        let body = format!(
            "{heading}\
             {intro}\
             {btn}\
             {fallback}\
             {expires}\
             {ignore}",
            heading = heading("Reset Your Password"),
            intro = paragraph("We received a request to reset the password for your Scrapix account."),
            btn = button(&reset_url, "Reset Password"),
            fallback = muted(&format!(
                "Or copy this link: <span style=\"color: #818cf8; word-break: break-all;\">{reset_url}</span>"
            )),
            expires = alert_box("This link expires in 1 hour.", AlertStyle::Warning),
            ignore = muted("If you didn't request a password reset, you can safely ignore this email. Your password will not be changed."),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: "Reset your password — Scrapix".to_string(),
            html: wrap("Reset Password", &body),
        });
    }

    // ------------------------------------------------------------------
    // 9. Password changed confirmation
    // ------------------------------------------------------------------
    pub fn send_password_changed(&self, to_email: &str) {
        let body = format!(
            "{heading}\
             {intro}\
             {btn}\
             {warn}",
            heading = heading("Password Changed"),
            intro = paragraph("Your Scrapix account password has been successfully changed."),
            btn = button(&format!("{CONSOLE_URL}/auth/login"), "Log In"),
            warn = muted(&format!(
                "If you didn't change your password, please contact us immediately at {}.",
                link("mailto:support@meilisearch.com", "support@meilisearch.com")
            )),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: "Your password has been changed — Scrapix".to_string(),
            html: wrap("Password Changed", &body),
        });
    }

    // ------------------------------------------------------------------
    // 10. Team invite
    // ------------------------------------------------------------------
    pub fn send_team_invite(
        &self,
        to_email: &str,
        account_name: &str,
        inviter_name: &str,
        role: &str,
        token: &str,
    ) {
        let account_name = html_escape(account_name);
        let inviter_name = html_escape(inviter_name);
        let invite_url = format!("{CONSOLE_URL}/invite?token={token}");
        let body = format!(
            "{heading}\
             {intro}\
             {ts}\
               {r1}\
               {r2}\
             {te}\
             {btn}\
             {fallback}\
             {ignore}",
            heading = heading("You're invited!"),
            intro = paragraph(&format!(
                "<strong style=\"color: #e4e4e7;\">{inviter_name}</strong> has invited you to join \
                 <strong style=\"color: #e4e4e7;\">{account_name}</strong> on Scrapix."
            )),
            ts = table_start(),
            r1 = kv_row("Account", &account_name, false),
            r2 = kv_row("Role", role, true),
            te = table_end(),
            btn = button(&invite_url, "Accept Invite"),
            fallback = muted(&format!(
                "Or copy this link: <span style=\"color: #818cf8; word-break: break-all;\">{invite_url}</span>"
            )),
            ignore = muted("This invite expires in 7 days. If you weren't expecting this, you can safely ignore it."),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: format!("You're invited to join {account_name} on Scrapix"),
            html: wrap("Team Invite", &body),
        });
    }

    // ------------------------------------------------------------------
    // 11. Low credit balance warning
    // ------------------------------------------------------------------
    pub fn send_low_balance_warning(&self, to_email: &str, current_balance: i64) {
        let body = format!(
            "{heading}\
             {intro}\
             {suggestion}\
             {btn}",
            heading = heading("Low Credit Balance"),
            intro = paragraph(&format!(
                "Your Scrapix account has <strong style=\"color: #fcd34d;\">{current_balance} credits</strong> remaining."
            )),
            suggestion = paragraph("To avoid interruptions to your crawl jobs, consider topping up your balance or enabling auto top-up."),
            btn = button(&format!("{CONSOLE_URL}/settings/billing"), "Add Credits"),
        );

        self.send_owned(OwnedSendEmailRequest {
            from: FROM_ADDRESS.to_string(),
            to: vec![to_email.to_string()],
            subject: format!("Low balance — {current_balance} credits remaining"),
            html: wrap("Low Balance", &body),
        });
    }
}

// ============================================================================
// Database helpers
// ============================================================================

/// Fetch the owner email for an account (always, for billing emails).
pub async fn get_account_email(pool: &sqlx::PgPool, account_id: uuid::Uuid) -> Option<String> {
    sqlx::query_scalar(
        "SELECT u.email FROM users u \
         JOIN account_members m ON m.user_id = u.id \
         WHERE m.account_id = $1 AND m.role = 'owner' \
         LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Fetch the owner email for an account, only if they opted into job notifications.
pub async fn get_account_email_for_job_notification(
    pool: &sqlx::PgPool,
    account_id: uuid::Uuid,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT u.email FROM users u \
         JOIN account_members m ON m.user_id = u.id \
         WHERE m.account_id = $1 AND m.role = 'owner' \
         AND u.notify_job_emails = true \
         LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

// ============================================================================
// String helpers
// ============================================================================

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
