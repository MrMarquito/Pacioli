#[cfg(test)]
mod tests {
    use chrono::Utc;
    use dotenvy::dotenv;
    use ledger_audit::verify_ledger_cryptographic_chain;
    use ledger_core::{AccountId, Currency, Direction, JournalEntryDraft, PostingDraft};
    use ledger_journal::execute_journal_entry;
    use ledger_outbox::drain_and_process_outbox_batch;
    use ledger_reconciliation::{
        reconcile_account, ExternalAccountStatement, ReconciliationStatus,
    };
    use rust_decimal_macros::dec;
    use sqlx::postgres::PgPoolOptions;
    use std::env;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    // Synchronizes tests within this file so they execute in sequence against the DB
    static TEST_MUTEX: Mutex<()> = Mutex::const_new(());

    async fn setup_pool() -> sqlx::PgPool {
        dotenv().ok();
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://ledger_admin:ledger_secure_password_2026@localhost:5433/ledger_db"
                .to_string()
        });

        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("Migrations failed");

        pool
    }

    async fn create_account(pool: &sqlx::PgPool, id: Uuid) {
        sqlx::query("INSERT INTO accounts (id, currency, status) VALUES ($1, 'USD', 'ACTIVE')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO account_balances (account_id, balance, currency) VALUES ($1, 0, 'USD')",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_cryptographic_audit_chain_verification() {
        let _guard = TEST_MUTEX.lock().await;
        let pool = setup_pool().await;

        let acc_a = AccountId(Uuid::now_v7());
        let acc_b = AccountId(Uuid::now_v7());
        create_account(&pool, acc_a.0).await;
        create_account(&pool, acc_b.0).await;

        // Post Entry 1
        let draft1 = JournalEntryDraft::new(
            format!("audit-1-{}", Uuid::now_v7()),
            "Entry 1".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc_a, Direction::Debit, dec!(100.00)).unwrap(),
                PostingDraft::new(acc_b, Direction::Credit, dec!(100.00)).unwrap(),
            ],
        )
        .unwrap();
        execute_journal_entry(&pool, draft1, b"p1").await.unwrap();

        // Post Entry 2
        let draft2 = JournalEntryDraft::new(
            format!("audit-2-{}", Uuid::now_v7()),
            "Entry 2".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc_a, Direction::Credit, dec!(25.00)).unwrap(),
                PostingDraft::new(acc_b, Direction::Debit, dec!(25.00)).unwrap(),
            ],
        )
        .unwrap();
        execute_journal_entry(&pool, draft2, b"p2").await.unwrap();

        // Audit check across the complete cryptographic chain
        let audit_report = verify_ledger_cryptographic_chain(&pool).await.unwrap();
        assert!(audit_report.is_valid);
        assert!(audit_report.entries_verified >= 2);
    }

    #[tokio::test]
    async fn test_outbox_drain_and_delivery() {
        let _guard = TEST_MUTEX.lock().await;
        let pool = setup_pool().await;

        let acc_a = AccountId(Uuid::now_v7());
        let acc_b = AccountId(Uuid::now_v7());
        create_account(&pool, acc_a.0).await;
        create_account(&pool, acc_b.0).await;

        let draft = JournalEntryDraft::new(
            format!("outbox-test-{}", Uuid::now_v7()),
            "Outbox Verification".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc_a, Direction::Debit, dec!(40.00)).unwrap(),
                PostingDraft::new(acc_b, Direction::Credit, dec!(40.00)).unwrap(),
            ],
        )
        .unwrap();

        execute_journal_entry(&pool, draft, b"outbox-p")
            .await
            .unwrap();

        let mut dispatched_count = 0;
        let processed = drain_and_process_outbox_batch(&pool, 50, |_record| {
            dispatched_count += 1;
            async { Ok(()) }
        })
        .await
        .unwrap();

        assert!(processed >= 1);
        assert!(dispatched_count >= 1);
    }

    #[tokio::test]
    async fn test_reconciliation_balanced_and_discrepancy() {
        let _guard = TEST_MUTEX.lock().await;
        let pool = setup_pool().await;

        let acc = AccountId(Uuid::now_v7());
        let offset = AccountId(Uuid::now_v7());
        create_account(&pool, acc.0).await;
        create_account(&pool, offset.0).await;

        let draft = JournalEntryDraft::new(
            format!("reconcile-test-{}", Uuid::now_v7()),
            "Funding".to_string(),
            Currency::USD,
            vec![
                PostingDraft::new(acc, Direction::Debit, dec!(200.00)).unwrap(),
                PostingDraft::new(offset, Direction::Credit, dec!(200.00)).unwrap(),
            ],
        )
        .unwrap();
        execute_journal_entry(&pool, draft, b"rec-p").await.unwrap();

        let matching_stmt = ExternalAccountStatement {
            account_id: acc,
            external_balance: dec!(200.00),
            statement_date: Utc::now(),
        };
        let rep1 = reconcile_account(&pool, &matching_stmt).await.unwrap();
        assert_eq!(rep1.status, ReconciliationStatus::Balanced);

        let mismatch_stmt = ExternalAccountStatement {
            account_id: acc,
            external_balance: dec!(180.00),
            statement_date: Utc::now(),
        };
        let rep2 = reconcile_account(&pool, &mismatch_stmt).await.unwrap();
        assert_eq!(
            rep2.status,
            ReconciliationStatus::Discrepancy {
                variance: dec!(20.00)
            }
        );
    }
}
