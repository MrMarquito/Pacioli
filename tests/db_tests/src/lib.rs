#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use sqlx::postgres::PgPoolOptions;
    use std::env;
    use uuid::Uuid;

    async fn get_connection_pool() -> sqlx::PgPool {
        dotenvy::dotenv().ok();
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://ledger_admin:ledger_secure_password_2026@localhost:5433/ledger_db"
                .to_string()
        });

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to PostgreSQL. Is the Docker container running?");

        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("Failed to apply database migrations");

        pool
    }

    #[tokio::test]
    async fn test_database_schema_migrations_and_immutability_triggers() {
        let pool = get_connection_pool().await;
        let mut tx = pool.begin().await.expect("Failed to begin transaction");

        let entry_id = Uuid::now_v7();
        let idempotency_key = format!("test-key-{}", entry_id);

        let mut hasher = Sha256::new();
        hasher.update(b"GENESIS_SEED_INVARIANT");
        hasher.update(entry_id.as_bytes());
        hasher.update(b"USD");
        let valid_hash = hasher.finalize().to_vec();

        // 1. Insert a journal entry inside the transaction
        sqlx::query(
            r#"
            INSERT INTO journal_entries (id, idempotency_key, description, currency, entry_hash)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(entry_id)
        .bind(&idempotency_key)
        .bind("Initial Seed Transaction")
        .bind("USD")
        .bind(&valid_hash)
        .execute(&mut *tx)
        .await
        .expect("Failed to insert valid journal entry");

        // 2. Test illegal UPDATE via savepoint
        sqlx::query("SAVEPOINT sp_update")
            .execute(&mut *tx)
            .await
            .unwrap();

        let update_result = sqlx::query(
            r#"
            UPDATE journal_entries
            SET description = 'Fraudulent alteration'
            WHERE id = $1
            "#,
        )
        .bind(entry_id)
        .execute(&mut *tx)
        .await;

        assert!(
            update_result.is_err(),
            "SECURITY FAILURE: PostgreSQL allowed an UPDATE mutation on journal_entries table!"
        );
        let update_err_message = update_result.unwrap_err().to_string();
        assert!(
            update_err_message.contains("CRITICAL INVARIANT VIOLATION"),
            "Expected immutability trigger message, received: {}",
            update_err_message
        );

        sqlx::query("ROLLBACK TO SAVEPOINT sp_update")
            .execute(&mut *tx)
            .await
            .unwrap();

        // 3. Test illegal DELETE via savepoint
        sqlx::query("SAVEPOINT sp_delete")
            .execute(&mut *tx)
            .await
            .unwrap();

        let delete_result = sqlx::query(
            r#"
            DELETE FROM journal_entries
            WHERE id = $1
            "#,
        )
        .bind(entry_id)
        .execute(&mut *tx)
        .await;

        assert!(
            delete_result.is_err(),
            "SECURITY FAILURE: PostgreSQL allowed a DELETE mutation on journal_entries table!"
        );
        let delete_err_message = delete_result.unwrap_err().to_string();
        assert!(
            delete_err_message.contains("CRITICAL INVARIANT VIOLATION"),
            "Expected immutability trigger message, received: {}",
            delete_err_message
        );

        sqlx::query("ROLLBACK TO SAVEPOINT sp_delete")
            .execute(&mut *tx)
            .await
            .unwrap();

        // Roll back transaction so test records never persist to the shared DB
        tx.rollback().await.expect("Failed to rollback transaction");
    }
}
