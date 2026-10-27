use ledger_balances::apply_balance_mutation;
use ledger_core::{AccountId, CoreError, JournalEntryDraft};
use ledger_idempotency::{
    acquire_or_verify_key, mark_key_committed, IdempotencyCheck, IdempotencyError,
};
use ledger_outbox::enqueue_event;
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JournalExecutionError {
    #[error("Core domain error: {0}")]
    Core(#[from] CoreError),

    #[error("Idempotency conflict: {0}")]
    Idempotency(#[from] IdempotencyError),

    #[error("Balance projection error: {0}")]
    Balance(#[from] ledger_balances::BalanceError),

    #[error("Outbox error: {0}")]
    Outbox(#[from] ledger_outbox::OutboxError),

    #[error("Account {0} does not exist or currency mismatch")]
    AccountInvalid(uuid::Uuid),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub enum CommitOutcome {
    Committed(uuid::Uuid),
    AlreadyProcessed(serde_json::Value),
}

/// Deterministically locks accounts in ascending order to prevent deadlocks.
async fn lock_accounts_deterministically(
    tx: &mut Transaction<'_, Postgres>,
    account_ids: &[AccountId],
    expected_currency: &str,
) -> Result<(), JournalExecutionError> {
    let mut sorted_ids: Vec<uuid::Uuid> = account_ids.iter().map(|a| a.0).collect();
    sorted_ids.sort_unstable();
    sorted_ids.dedup();

    for id in sorted_ids {
        let row = sqlx::query(
            r#"
            SELECT status, currency 
            FROM accounts 
            WHERE id = $1 
            FOR UPDATE
            "#,
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        match row {
            Some(r) => {
                let status: String = r.get("status");
                let currency: String = r.get("currency");

                if status != "ACTIVE" {
                    return Err(JournalExecutionError::Core(CoreError::AccountInactive(id)));
                }

                if currency != expected_currency {
                    return Err(JournalExecutionError::Core(CoreError::CurrencyMismatch {
                        expected: expected_currency.to_string(),
                        found: currency,
                    }));
                }
            }
            None => return Err(JournalExecutionError::AccountInvalid(id)),
        }
    }

    Ok(())
}

/// Executes complete double-entry transaction within a single ACID transaction.
pub async fn execute_journal_entry(
    pool: &PgPool,
    draft: JournalEntryDraft,
    raw_payload_bytes: &[u8],
) -> Result<CommitOutcome, JournalExecutionError> {
    let mut tx = pool.begin().await?;

    // 1. Payload-aware Idempotency Verification
    match acquire_or_verify_key(&mut tx, &draft.idempotency_key, raw_payload_bytes).await? {
        IdempotencyCheck::AlreadyCommitted { response_body, .. } => {
            tx.rollback().await?;
            return Ok(CommitOutcome::AlreadyProcessed(
                response_body.unwrap_or(json!({ "status": "COMMITTED" })),
            ));
        }
        IdempotencyCheck::Proceed => {}
    }

    // 2. Deterministic Row Locking
    let account_ids: Vec<AccountId> = draft.postings.iter().map(|p| p.account_id).collect();
    let currency_str = draft.currency.to_string();
    lock_accounts_deterministically(&mut tx, &account_ids, &currency_str).await?;

    // 3. Cryptographic Chaining
    // Transaction-level advisory lock serializes sequence assignment and hash calculation
    sqlx::query("SELECT pg_advisory_xact_lock(72109283)")
        .execute(&mut *tx)
        .await?;

    let last_entry = sqlx::query(
        r#"
        SELECT entry_hash 
        FROM journal_entries 
        ORDER BY seq DESC 
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let previous_hash = last_entry.map(|r| r.get::<Vec<u8>, _>("entry_hash"));
    let entry_hash = ledger_core::compute_entry_hash(
        previous_hash.as_deref(),
        draft.id.0,
        &currency_str,
        &draft.postings,
    );

    // 4. Insert Journal Entry Envelope (seq auto-generated)
    sqlx::query(
        r#"
        INSERT INTO journal_entries (id, idempotency_key, description, currency, previous_entry_hash, entry_hash)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(draft.id.0)
    .bind(&draft.idempotency_key)
    .bind(&draft.description)
    .bind(&currency_str)
    .bind(previous_hash)
    .bind(&entry_hash)
    .execute(&mut *tx)
    .await?;

    // 5. Insert Postings & Mutate Materialized Balances
    for (idx, p) in draft.postings.iter().enumerate() {
        let posting_id = uuid::Uuid::now_v7();
        let direction_str = match p.direction {
            ledger_core::Direction::Debit => "DEBIT",
            ledger_core::Direction::Credit => "CREDIT",
        };

        sqlx::query(
            r#"
            INSERT INTO postings (id, entry_id, account_id, amount, direction, sequence_num)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(posting_id)
        .bind(draft.id.0)
        .bind(p.account_id.0)
        .bind(p.amount)
        .bind(direction_str)
        .bind(idx as i32)
        .execute(&mut *tx)
        .await?;

        apply_balance_mutation(&mut tx, p.account_id, p.direction, p.amount, draft.id).await?;
    }

    // 6. Transactional Outbox Notification
    let event_payload = json!({
        "entry_id": draft.id.0,
        "status": "COMMITTED",
        "idempotency_key": draft.idempotency_key,
        "currency": currency_str,
        "postings_count": draft.postings.len()
    });

    enqueue_event(
        &mut tx,
        "JOURNAL_ENTRY",
        draft.id.0,
        "JOURNAL_ENTRY_COMMITTED",
        event_payload.clone(),
    )
    .await?;

    // 7. Commit Idempotency Record
    mark_key_committed(&mut tx, &draft.idempotency_key, 200, &event_payload).await?;

    // Final Commit of ACID transaction
    tx.commit().await?;

    Ok(CommitOutcome::Committed(draft.id.0))
}
