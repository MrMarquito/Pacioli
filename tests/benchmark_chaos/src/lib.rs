#[cfg(test)]
mod tests {
    use dotenvy::dotenv;
    use ledger_accounts::create_account;
    use ledger_audit::verify_ledger_cryptographic_chain;
    use ledger_core::{Currency, Direction, JournalEntryDraft, PostingDraft};
    use ledger_journal::execute_journal_entry;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::Row;
    use std::env;
    use std::sync::Arc;
    use std::time::Instant;
    use uuid::Uuid;

    async fn setup_pool() -> Arc<sqlx::PgPool> {
        dotenv().ok();
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://ledger_admin:ledger_secure_password_2026@localhost:5433/ledger_db"
                .to_string()
        });

        let pool = PgPoolOptions::new()
            .max_connections(30)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database");

        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("Failed to apply migrations");

        Arc::new(pool)
    }

    #[tokio::test]
    async fn benchmark_hot_account_contention_and_latency() {
        let pool = setup_pool().await;

        let merchant = create_account(&pool, None, Currency::USD).await.unwrap();
        let customer = create_account(&pool, None, Currency::USD).await.unwrap();

        let iterations = 100;
        let mut handles = Vec::new();
        let start_time = Instant::now();

        for i in 0..iterations {
            let pool_clone = Arc::clone(&pool);
            let m_id = merchant.id;
            let c_id = customer.id;

            let handle = tokio::spawn(async move {
                let tx_start = Instant::now();
                let key = format!("bench-{}-{}", i, Uuid::now_v7());
                let payload = format!("bench-payload-{}", i).into_bytes();

                let draft = JournalEntryDraft::new(
                    key,
                    format!("Bench Transfer {}", i),
                    Currency::USD,
                    vec![
                        PostingDraft::new(c_id, Direction::Debit, dec!(1.50)).unwrap(),
                        PostingDraft::new(m_id, Direction::Credit, dec!(1.50)).unwrap(),
                    ],
                )
                .unwrap();

                let res = execute_journal_entry(&pool_clone, draft, &payload).await;
                (res, tx_start.elapsed())
            });

            handles.push(handle);
        }

        let mut latencies = Vec::new();
        for handle in handles {
            let (res, duration) = handle.await.expect("Task failed");
            assert!(res.is_ok(), "Contention failure: {:?}", res.err());
            latencies.push(duration.as_millis());
        }

        let total_duration = start_time.elapsed();
        latencies.sort_unstable();

        let p50 = latencies[latencies.len() / 2];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];
        let tps = (iterations as f64) / total_duration.as_secs_f64();

        println!("\n=== HOT ACCOUNT BENCHMARK RESULTS ===");
        println!("Total Transactions : {}", iterations);
        println!("Elapsed Time       : {:?}", total_duration);
        println!("Throughput (TPS)   : {:.2} tx/sec", tps);
        println!("p50 Latency        : {} ms", p50);
        println!("p99 Latency        : {} ms", p99);
        println!("====================================\n");

        assert!(tps > 10.0, "Throughput fell below baseline requirement");
    }

    #[tokio::test]
    async fn chaos_transaction_mid_flight_failure_integrity() {
        let pool = setup_pool().await;

        let acc_a = create_account(&pool, None, Currency::USD).await.unwrap();
        let _acc_b = create_account(&pool, None, Currency::USD).await.unwrap();

        let initial_row_a =
            sqlx::query("SELECT balance FROM account_balances WHERE account_id = $1")
                .bind(acc_a.id.0)
                .fetch_one(&*pool)
                .await
                .unwrap();
        let initial_balance_a: Decimal = initial_row_a.get("balance");

        // Simulate failure injection: Start a transaction, lock accounts, mutate, then abort
        let mut tx = pool.begin().await.unwrap();

        sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE")
            .bind(acc_a.id.0)
            .fetch_one(&mut *tx)
            .await
            .unwrap();

        sqlx::query(
            "UPDATE account_balances SET balance = balance + 9999.00 WHERE account_id = $1",
        )
        .bind(acc_a.id.0)
        .execute(&mut *tx)
        .await
        .unwrap();

        // Injected chaos: Worker disconnects / rolls back without committing
        tx.rollback().await.unwrap();

        // Assert that the balance never leaked the uncommitted phantom mutation
        let final_row_a = sqlx::query("SELECT balance FROM account_balances WHERE account_id = $1")
            .bind(acc_a.id.0)
            .fetch_one(&*pool)
            .await
            .unwrap();
        let final_balance_a: Decimal = final_row_a.get("balance");
        assert_eq!(initial_balance_a, final_balance_a);

        // Verify that the cryptographic audit chain remained intact
        let audit = verify_ledger_cryptographic_chain(&pool).await.unwrap();
        assert!(audit.is_valid);
    }
}
