use ledger_core::{Direction, PostingDraft};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AuditError {
    #[error("Database error during audit verification: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Tamper detected at entry {entry_id}: expected hash {expected}, found {actual}")]
    ChainBroken {
        entry_id: Uuid,
        expected: String,
        actual: String,
    },

    #[error("Corrupted chain link at entry {entry_id}: previous hash does not match prior record")]
    BrokenLink {
        entry_id: Uuid,
        expected_prev: String,
        actual_prev: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub entries_verified: usize,
    pub is_valid: bool,
}

pub async fn verify_ledger_cryptographic_chain(pool: &PgPool) -> Result<AuditReport, AuditError> {
    let entries = sqlx::query(
        r#"
        SELECT id, currency, previous_entry_hash, entry_hash
        FROM journal_entries
        ORDER BY seq ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut expected_previous_hash: Option<Vec<u8>> = None;

    for row in &entries {
        let entry_id: Uuid = row.get("id");
        let currency: String = row.get("currency");
        let stored_prev_hash: Option<Vec<u8>> = row.get("previous_entry_hash");
        let stored_entry_hash: Vec<u8> = row.get("entry_hash");

        // Verify link to predecessor
        if stored_prev_hash != expected_previous_hash {
            return Err(AuditError::BrokenLink {
                entry_id,
                expected_prev: hex::encode(expected_previous_hash.unwrap_or_default()),
                actual_prev: hex::encode(stored_prev_hash.unwrap_or_default()),
            });
        }

        // Fetch postings for this entry
        let posting_rows = sqlx::query(
            r#"
            SELECT account_id, amount, direction
            FROM postings
            WHERE entry_id = $1
            ORDER BY sequence_num ASC
            "#,
        )
        .bind(entry_id)
        .fetch_all(pool)
        .await?;

        let postings: Vec<PostingDraft> = posting_rows
            .into_iter()
            .map(|pr| {
                let acc: Uuid = pr.get("account_id");
                let dir_str: String = pr.get("direction");
                let direction = match dir_str.as_str() {
                    "DEBIT" => Direction::Debit,
                    _ => Direction::Credit,
                };
                PostingDraft {
                    account_id: ledger_core::AccountId(acc),
                    direction,
                    amount: pr.get("amount"),
                }
            })
            .collect();

        // Recompute hash using the canonical shared function in ledger-core
        let computed = ledger_core::compute_entry_hash(
            stored_prev_hash.as_deref(),
            entry_id,
            &currency,
            &postings,
        );

        if computed != stored_entry_hash {
            return Err(AuditError::ChainBroken {
                entry_id,
                expected: hex::encode(computed),
                actual: hex::encode(stored_entry_hash),
            });
        }

        expected_previous_hash = Some(stored_entry_hash);
    }

    Ok(AuditReport {
        entries_verified: entries.len(),
        is_valid: true,
    })
}

mod hex {
    pub fn encode(data: impl AsRef<[u8]>) -> String {
        data.as_ref().iter().map(|b| format!("{:02x}", b)).collect()
    }
}
