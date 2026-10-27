use ledger_core::AccountId;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReconciliationError {
    #[error("Database error during reconciliation: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Account {0} has no derived balance record in ledger")]
    AccountNotFound(uuid::Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalAccountStatement {
    pub account_id: AccountId,
    pub external_balance: Decimal,
    pub statement_date: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconciliationStatus {
    Balanced,
    Discrepancy { variance: Decimal },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub account_id: AccountId,
    pub internal_balance: Decimal,
    pub external_balance: Decimal,
    pub status: ReconciliationStatus,
}

pub async fn reconcile_account(
    pool: &PgPool,
    statement: &ExternalAccountStatement,
) -> Result<ReconciliationReport, ReconciliationError> {
    let row = sqlx::query(
        r#"
        SELECT balance
        FROM account_balances
        WHERE account_id = $1
        "#,
    )
    .bind(statement.account_id.0)
    .fetch_optional(pool)
    .await?;

    let internal_balance: Decimal = match row {
        Some(r) => r.get("balance"),
        None => return Err(ReconciliationError::AccountNotFound(statement.account_id.0)),
    };

    let variance = internal_balance - statement.external_balance;

    let status = if variance.is_zero() {
        ReconciliationStatus::Balanced
    } else {
        ReconciliationStatus::Discrepancy { variance }
    };

    Ok(ReconciliationReport {
        account_id: statement.account_id,
        internal_balance,
        external_balance: statement.external_balance,
        status,
    })
}
