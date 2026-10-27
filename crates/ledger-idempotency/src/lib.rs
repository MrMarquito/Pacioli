use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IdempotencyError {
    #[error("Idempotency key '{key}' was reused with a different request payload")]
    PayloadMismatch { key: String },

    #[error("Operation for key '{key}' is currently concurrently processing")]
    ConcurrentConflict { key: String },

    #[error("Database error during idempotency check: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

pub enum IdempotencyCheck {
    Proceed,
    AlreadyCommitted {
        response_code: Option<i32>,
        response_body: Option<serde_json::Value>,
    },
}

pub fn hash_payload(payload: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    hasher.finalize().to_vec()
}

pub async fn acquire_or_verify_key(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
    payload_bytes: &[u8],
) -> Result<IdempotencyCheck, IdempotencyError> {
    let calculated_hash = hash_payload(payload_bytes);

    let existing = sqlx::query(
        r#"
        SELECT payload_hash, status, response_code, response_body
        FROM idempotency_keys
        WHERE key = $1
        FOR UPDATE
        "#,
    )
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;

    match existing {
        Some(row) => {
            use sqlx::Row;
            let stored_hash: Vec<u8> = row.get("payload_hash");
            let status: String = row.get("status");

            if stored_hash != calculated_hash {
                return Err(IdempotencyError::PayloadMismatch {
                    key: key.to_string(),
                });
            }

            match status.as_str() {
                "COMMITTED" => {
                    let code: Option<i32> = row.get("response_code");
                    let body: Option<serde_json::Value> = row.get("response_body");
                    Ok(IdempotencyCheck::AlreadyCommitted {
                        response_code: code,
                        response_body: body,
                    })
                }
                "PROCESSING" => Err(IdempotencyError::ConcurrentConflict {
                    key: key.to_string(),
                }),
                _ => Err(IdempotencyError::PayloadMismatch {
                    key: key.to_string(),
                }),
            }
        }
        None => {
            sqlx::query(
                r#"
                INSERT INTO idempotency_keys (key, payload_hash, status)
                VALUES ($1, $2, 'PROCESSING')
                "#,
            )
            .bind(key)
            .bind(&calculated_hash)
            .execute(&mut **tx)
            .await?;

            Ok(IdempotencyCheck::Proceed)
        }
    }
}

pub async fn mark_key_committed(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
    response_code: i32,
    response_body: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE idempotency_keys
        SET status = 'COMMITTED',
            response_code = $2,
            response_body = $3,
            updated_at = NOW()
        WHERE key = $1
        "#,
    )
    .bind(key)
    .bind(response_code)
    .bind(response_body)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
