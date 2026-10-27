use ledger_core::{AccountId, Direction, EntryId};
use rust_decimal::Decimal;
use sqlx::{Postgres, Transaction};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BalanceError {
    #[error("Balance projection not initialized for account {0}")]
    AccountNotFound(uuid::Uuid),

    #[error("Database error while applying balance update: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

pub async fn apply_balance_mutation(
    tx: &mut Transaction<'_, Postgres>,
    account_id: AccountId,
    direction: Direction,
    amount: Decimal,
    entry_id: EntryId,
) -> Result<(), BalanceError> {
    let delta = match direction {
        Direction::Debit => amount,
        Direction::Credit => -amount,
    };

    let result = sqlx::query(
        r#"
        UPDATE account_balances
        SET balance = balance + $2,
            version = version + 1,
            last_entry_id = $3,
            updated_at = NOW()
        WHERE account_id = $1
        "#,
    )
    .bind(account_id.0)
    .bind(delta)
    .bind(entry_id.0)
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(BalanceError::AccountNotFound(account_id.0));
    }

    Ok(())
}
