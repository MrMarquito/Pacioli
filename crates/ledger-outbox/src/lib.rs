use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum OutboxError {
    #[error("Database error while managing outbox: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct OutboxRecord {
    pub id: Uuid,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub payload: Value,
}

pub async fn enqueue_event(
    tx: &mut Transaction<'_, Postgres>,
    aggregate_type: &str,
    aggregate_id: Uuid,
    event_type: &str,
    payload: Value,
) -> Result<(), OutboxError> {
    sqlx::query(
        r#"
        INSERT INTO outbox_events (aggregate_type, aggregate_id, event_type, payload)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Drains a batch of events using FOR UPDATE SKIP LOCKED to guarantee concurrency safety.
pub async fn drain_and_process_outbox_batch<F, Fut>(
    pool: &PgPool,
    batch_size: i64,
    mut handler: F,
) -> Result<usize, OutboxError>
where
    F: FnMut(OutboxRecord) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut tx = pool.begin().await?;

    let rows = sqlx::query(
        r#"
        SELECT id, aggregate_type, aggregate_id, event_type, payload
        FROM outbox_events
        WHERE processed_at IS NULL
        ORDER BY created_at ASC
        LIMIT $1
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .bind(batch_size)
    .fetch_all(&mut *tx)
    .await?;

    let mut processed_ids = Vec::new();

    for row in rows {
        let record = OutboxRecord {
            id: row.get("id"),
            aggregate_type: row.get("aggregate_type"),
            aggregate_id: row.get("aggregate_id"),
            event_type: row.get("event_type"),
            payload: row.get("payload"),
        };

        if handler(record.clone()).await.is_ok() {
            processed_ids.push(record.id);
        }
    }

    if !processed_ids.is_empty() {
        sqlx::query(
            r#"
            UPDATE outbox_events
            SET processed_at = NOW()
            WHERE id = ANY($1)
            "#,
        )
        .bind(&processed_ids)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(processed_ids.len())
}
