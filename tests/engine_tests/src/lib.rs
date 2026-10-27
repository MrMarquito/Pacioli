#[cfg(test)]
mod tests {
    use dotenvy::dotenv;
    use ledger_core::{AccountId, Currency, Direction, JournalEntryDraft, PostingDraft};
    use ledger_journal::{execute_journal_entry, CommitOutcome, JournalExecutionError};
    use rust_decimal_macros::dec;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;
    use std::env;
    use std::sync::Arc;
    use uuid::Uuid;

    async fn setup_db_pool() -> sqlx::PgPool {
        dotenv().ok();
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://ledger_admin:ledger_secure_password_2026@localhost:5433/ledger_db"
                .to_string()
        });

        let pool = PgPoolOptions::new()
            .max_connections(25)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("Failed to apply migrations");

        pool
    }

    async fn create_test_account(pool: &sqlx::PgPool, id: Uuid, currency: &str) {
        sqlx::query("INSERT INTO accounts (id, currency, status) VALUES ($1, $2, 'ACTIVE')")
            .bind(id)
            .bind(currency)
            .execute(pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO account_balances (account_id, balance, currency) VALUES ($1, 0, $2)",
        )
        .bind(id)
        .bind(currency)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_idempotency_cache_and_payload_tampering() {
        let pool = setup_db_pool().await;
        let acc_a = AccountId(Uuid::now_v7());
        let acc_b = AccountId(Uuid::now_v7());
        create_test_account(&pool, acc_a.0, "USD").await;
        create_test_account(&pool, acc_b.0, "USD").await;

        let key = format!("idemp-test-{}", Uuid::now_v7());
        let payload_1 = b"Original Transfer $50";

        let draft_1 = JournalEntryDraft::new(
            key.clone(),
            "Transfer 1".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc_a, Direction::Debit, dec!(50.00)).unwrap(),
                PostingDraft::new(acc_b, Direction::Credit, dec!(50.00)).unwrap(),
            ],
        )
        .unwrap();

        // First Execution: Must commit
        let res_1 = execute_journal_entry(&pool, draft_1, payload_1).await;
        assert!(matches!(res_1, Ok(CommitOutcome::Committed(_))));

        // Second Execution (Same Key, Same Payload): Must return cached outcome
        let draft_retry = JournalEntryDraft::new(
            key.clone(),
            "Transfer 1 Retry".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc_a, Direction::Debit, dec!(50.00)).unwrap(),
                PostingDraft::new(acc_b, Direction::Credit, dec!(50.00)).unwrap(),
            ],
        )
        .unwrap();

        let res_retry = execute_journal_entry(&pool, draft_retry, payload_1).await;
        assert!(matches!(res_retry, Ok(CommitOutcome::AlreadyProcessed(_))));

        // Third Execution (Same Key, Altered Payload): Must fail with payload mismatch
        let payload_tampered = b"Tampered Transfer $1000";
        let draft_tampered = JournalEntryDraft::new(
            key.clone(),
            "Tampered Transfer".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc_a, Direction::Debit, dec!(1000.00)).unwrap(),
                PostingDraft::new(acc_b, Direction::Credit, dec!(1000.00)).unwrap(),
            ],
        )
        .unwrap();

        let res_tampered = execute_journal_entry(&pool, draft_tampered, payload_tampered).await;
        assert!(matches!(
            res_tampered,
            Err(JournalExecutionError::Idempotency(_))
        ));
    }

    #[tokio::test]
    async fn test_deadlock_free_concurrent_bi_directional_transfers() {
        let pool = Arc::new(setup_db_pool().await);
        let acc_x = AccountId(Uuid::now_v7());
        let acc_y = AccountId(Uuid::now_v7());

        create_test_account(&pool, acc_x.0, "USD").await;
        create_test_account(&pool, acc_y.0, "USD").await;

        let num_tasks = 20;
        let mut handles = Vec::new();

        // Concurrently run 20 transfers where odd tasks do X -> Y and even tasks do Y -> X
        for i in 0..num_tasks {
            let pool_clone = Arc::clone(&pool);
            let handle = tokio::spawn(async move {
                let (from, to) = if i % 2 == 0 {
                    (acc_x, acc_y)
                } else {
                    (acc_y, acc_x)
                };

                let key = format!("concurrent-key-{}-{}", i, Uuid::now_v7());
                let payload = format!("payload-{}", i).into_bytes();

                let draft = JournalEntryDraft::new(
                    key,
                    format!("Concurrent Transfer {}", i),
                    Currency::USD,
                    vec![
                        PostingDraft::new(from, Direction::Debit, dec!(10.00)).unwrap(),
                        PostingDraft::new(to, Direction::Credit, dec!(10.00)).unwrap(),
                    ],
                )
                .unwrap();

                execute_journal_entry(&pool_clone, draft, &payload).await
            });
            handles.push(handle);
        }

        for handle in handles {
            let res = handle.await.expect("Task panicked");
            assert!(
                res.is_ok(),
                "Deterministic lock ordering failed to prevent deadlock: {:?}",
                res.err()
            );
        }

        // Verify balance projection consistency:
        // 10 tasks debited X by 10 (+100) and credited X by 10 (-100). Net balance must equal 0.0000
        let row_x = sqlx::query("SELECT balance FROM account_balances WHERE account_id = $1")
            .bind(acc_x.0)
            .fetch_one(&*pool)
            .await
            .unwrap();

        let balance_x: rust_decimal::Decimal = row_x.get("balance");
        assert_eq!(balance_x, dec!(0.0000));
    }
}
