//! Backfill `accounts.hyperline_customer_id` for rows that survived the
//! SCR-68 Stripe → Hyperline migration with NULL.
//!
//! Walks every account where `hyperline_customer_id IS NULL`, looks up
//! the oldest 'owner' membership for `(name, email)`, and either reuses
//! an existing Hyperline customer (matched by `external_id =
//! accounts.id`) or creates a new one — then writes the id back.
//!
//! Safe to re-run: each step is idempotent. Failures are logged per
//! account and the loop continues; rerun to retry only the survivors.
//!
//! ```sh
//! DATABASE_URL=postgres://… \
//! HYPERLINE_API_KEY=prod_… \
//!   cargo run -p scrapix-api --bin backfill_hyperline_customers
//! ```

use std::error::Error;

use scrapix_billing_hyperline::HyperlineClient;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use tracing::{error, info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")?;
    let url = if !database_url.contains("sslmode=") {
        let sep = if database_url.contains('?') { "&" } else { "?" };
        format!("{database_url}{sep}sslmode=require")
    } else {
        database_url
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await?;

    let client = HyperlineClient::from_env()?;
    let target = if client.config().is_sandbox() {
        "sandbox"
    } else {
        "PRODUCTION"
    };
    info!("Backfilling Hyperline customers into {target}…");

    // We page through accounts; new ones get linked as the loop runs so
    // restarting from the top each iteration would be wasteful. Capture
    // the snapshot once and walk it.
    let rows = sqlx::query(
        "SELECT a.id AS account_id, a.name AS account_name, u.email AS owner_email \
         FROM accounts a \
         JOIN account_members m ON m.account_id = a.id AND m.role = 'owner' \
         JOIN users u ON u.id = m.user_id \
         WHERE a.hyperline_customer_id IS NULL \
         GROUP BY a.id, a.name, u.email, m.joined_at \
         ORDER BY a.id, m.joined_at ASC",
    )
    .fetch_all(&pool)
    .await?;

    // GROUP BY + ORDER BY gives duplicates per account if multiple
    // owners. Dedup by account_id, keeping first (= oldest owner).
    let mut seen = std::collections::HashSet::new();
    let mut targets: Vec<(Uuid, String, String)> = Vec::new();
    for row in rows {
        let id: Uuid = row.get("account_id");
        if !seen.insert(id) {
            continue;
        }
        targets.push((id, row.get("account_name"), row.get("owner_email")));
    }

    info!("Found {} unlinked account(s)", targets.len());

    let mut linked = 0usize;
    let mut reused = 0usize;
    let mut failed = 0usize;

    for (account_id, account_name, owner_email) in targets {
        match link_one(&pool, &client, account_id, &account_name, &owner_email).await {
            Ok(LinkOutcome::Created) => {
                linked += 1;
                info!(account_id = %account_id, "linked (created in Hyperline)");
            }
            Ok(LinkOutcome::Reused) => {
                reused += 1;
                info!(account_id = %account_id, "linked (reused existing Hyperline customer)");
            }
            Err(e) => {
                failed += 1;
                error!(account_id = %account_id, error = %e, "link failed");
            }
        }
    }

    info!("Done. created={linked} reused={reused} failed={failed}");
    if failed > 0 {
        warn!("{failed} account(s) still unlinked — rerun after addressing the root cause");
        std::process::exit(1);
    }
    Ok(())
}

enum LinkOutcome {
    Created,
    Reused,
}

async fn link_one(
    pool: &PgPool,
    client: &HyperlineClient,
    account_id: Uuid,
    account_name: &str,
    owner_email: &str,
) -> Result<LinkOutcome, Box<dyn Error>> {
    let external_id = account_id.to_string();

    let (customer_id, outcome) = match client.find_customer_by_external_id(&external_id).await? {
        Some(existing) => (existing.id, LinkOutcome::Reused),
        None => {
            let created = client
                .create_customer(&external_id, account_name, owner_email)
                .await?;
            (created.id, LinkOutcome::Created)
        }
    };

    sqlx::query(
        "UPDATE accounts \
         SET hyperline_customer_id = COALESCE(hyperline_customer_id, $1) \
         WHERE id = $2",
    )
    .bind(&customer_id)
    .bind(account_id)
    .execute(pool)
    .await?;

    Ok(outcome)
}
