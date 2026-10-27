use chrono::{DateTime, Utc};
use ledger_core::{AccountId, Currency};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AccountError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Account {0} already exists")]
    AlreadyExists(Uuid),

    #[error("Account {0} not found")]
    NotFound(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub currency: Currency,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

pub async fn create_account(
    pool: &PgPool,
    id: Option<AccountId>,
    currency: Currency,
) -> Result<Account, AccountError> {
    let account_id = id.unwrap_or_else(AccountId::new_v7);
    let currency_str = currency.to_string();
    let now = Utc::now();

    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO accounts (id, currency, status, created_at, updated_at)
        VALUES ($1, $2, 'ACTIVE', $3, $3)
        "#,
    )
    .bind(account_id.0)
    .bind(&currency_str)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO account_balances (account_id, balance, currency, version, updated_at)
        VALUES ($1, 0.0000, $2, 0, $3)
        "#,
    )
    .bind(account_id.0)
    .bind(&currency_str)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Account {
        id: account_id,
        currency,
        status: "ACTIVE".to_string(),
        created_at: now,
    })
}

pub async fn get_account(pool: &PgPool, id: AccountId) -> Result<Account, AccountError> {
    let row = sqlx::query(
        r#"
        SELECT id, currency, status, created_at
        FROM accounts
        WHERE id = $1
        "#,
    )
    .bind(id.0)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => {
            let curr_str: String = r.get("currency");
            let currency = match curr_str.as_str() {
                "EUR" => Currency::EUR,
                "GBP" => Currency::GBP,
                "JPY" => Currency::JPY,
                "CAD" => Currency::CAD,
                _ => Currency::USD,
            };

            Ok(Account {
                id: AccountId(r.get("id")),
                currency,
                status: r.get("status"),
                created_at: r.get("created_at"),
            })
        }
        None => Err(AccountError::NotFound(id.0)),
    }
}
