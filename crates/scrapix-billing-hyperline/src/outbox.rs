//! Background drain worker for the Hyperline usage-event outbox.
//!
//! One tokio task loops every `interval`, pulls the oldest unsent rows whose
//! account already has a `hyperline_customer_id`, POSTs them to the ingest
//! API, and marks `sent_at` on success (or bumps `attempts` + `last_error`
//! on failure). The outbox row UUID doubles as Hyperline's `record_id`, so
//! duplicate deliveries are deduped server-side.

use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::client::HyperlineClient;
use crate::error::HyperlineError;
use crate::events::{build_record, OutboxPayload, UsageEvent};

/// Max attempts per row before we stop retrying. Further retries require a
/// manual replay (reset `attempts` / `sent_at` in the DB).
const MAX_ATTEMPTS: i32 = 10;

#[derive(Debug, Default, Clone, Copy)]
pub struct DrainStats {
    pub succeeded: u32,
    pub failed: u32,
    pub skipped: u32,
}

struct OutboxRow {
    id: Uuid,
    hyperline_customer_id: String,
    event_type: String,
    payload: serde_json::Value,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl OutboxRow {
    fn from_pg(row: PgRow) -> Result<Self, HyperlineError> {
        Ok(Self {
            id: row.try_get("id")?,
            hyperline_customer_id: row.try_get("hyperline_customer_id")?,
            event_type: row.try_get("event_type")?,
            payload: row.try_get("payload")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Drain up to `batch_size` outbox rows once. Intended for both the
/// background worker and ops/replay tooling.
pub async fn drain_once(
    pool: &PgPool,
    client: &HyperlineClient,
    batch_size: i64,
) -> Result<DrainStats, HyperlineError> {
    let rows = sqlx::query(
        "SELECT o.id, o.event_type, o.payload, o.created_at, \
                a.hyperline_customer_id \
         FROM hyperline_events_outbox o \
         JOIN accounts a ON a.id = o.account_id \
         WHERE o.sent_at IS NULL \
           AND o.attempts < $1 \
           AND a.hyperline_customer_id IS NOT NULL \
         ORDER BY o.created_at ASC \
         LIMIT $2",
    )
    .bind(MAX_ATTEMPTS)
    .bind(batch_size)
    .fetch_all(pool)
    .await?;

    let mut stats = DrainStats::default();
    for raw in rows {
        let row = OutboxRow::from_pg(raw)?;
        match deliver_row(pool, client, &row).await {
            Ok(()) => stats.succeeded += 1,
            Err(DrainError::Permanent(e)) => {
                stats.failed += 1;
                error!(id = %row.id, error = %e, "outbox row permanently failed");
            }
            Err(DrainError::Transient(e)) => {
                stats.failed += 1;
                warn!(id = %row.id, error = %e, "outbox row transient failure — will retry");
            }
        }
    }
    debug!(?stats, "outbox drain pass complete");
    Ok(stats)
}

/// Spawn the drain loop as a tokio task. Returns the `JoinHandle` so callers
/// can `abort()` on shutdown.
pub fn spawn_drain_worker(
    pool: PgPool,
    client: HyperlineClient,
    interval: Duration,
    batch_size: i64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Align to the interval start, skipping missed ticks under load.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(e) = drain_once(&pool, &client, batch_size).await {
                error!(error = %e, "outbox drain pass failed");
            }
        }
    })
}

enum DrainError {
    Transient(HyperlineError),
    Permanent(HyperlineError),
}

async fn deliver_row(
    pool: &PgPool,
    client: &HyperlineClient,
    row: &OutboxRow,
) -> Result<(), DrainError> {
    let payload: OutboxPayload = match serde_json::from_value(row.payload.clone()) {
        Ok(p) => p,
        Err(e) => {
            // Malformed payload won't fix itself on retry — drop it.
            mark_failed(pool, row.id, &e.to_string(), /* final */ true).await;
            return Err(DrainError::Permanent(HyperlineError::Decode(e)));
        }
    };

    let id_string = row.id.to_string();
    let record = build_record(&id_string, payload.quantity, payload.metadata.as_ref());
    let event = UsageEvent {
        customer_id: &row.hyperline_customer_id,
        event_type: &row.event_type,
        timestamp: row.created_at,
        record,
    };

    match client.ingest_event(&event).await {
        Ok(()) => {
            mark_sent(pool, row.id)
                .await
                .map_err(DrainError::Permanent)?;
            Ok(())
        }
        Err(e) => {
            let permanent = matches!(
                &e,
                HyperlineError::Api { status, .. } if *status >= 400 && *status < 500 && *status != 429
            );
            mark_failed(pool, row.id, &e.to_string(), permanent).await;
            if permanent {
                Err(DrainError::Permanent(e))
            } else {
                Err(DrainError::Transient(e))
            }
        }
    }
}

async fn mark_sent(pool: &PgPool, id: Uuid) -> Result<(), HyperlineError> {
    sqlx::query("UPDATE hyperline_events_outbox SET sent_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn mark_failed(pool: &PgPool, id: Uuid, err: &str, permanent: bool) {
    // On a permanent 4xx we push `attempts` to MAX_ATTEMPTS so the query
    // filter excludes the row from future picks; transient bumps by 1.
    let result = if permanent {
        sqlx::query(
            "UPDATE hyperline_events_outbox \
             SET attempts = $1, last_error = $2 \
             WHERE id = $3",
        )
        .bind(MAX_ATTEMPTS)
        .bind(err)
        .bind(id)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            "UPDATE hyperline_events_outbox \
             SET attempts = attempts + 1, last_error = $1 \
             WHERE id = $2",
        )
        .bind(err)
        .bind(id)
        .execute(pool)
        .await
    };
    if let Err(e) = result {
        error!(id = %id, error = %e, "failed to update outbox row failure state");
    }
}
