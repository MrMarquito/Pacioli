mod routes;
mod telemetry;

use axum::routing::{get, post};
use axum::Router;
use routes::{
    handle_audit_verification, handle_create_account, handle_create_transaction,
    handle_get_account_balance, handle_reconciliation, AppState,
};
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use telemetry::init_telemetry;
use tower_http::trace::TraceLayer;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    init_telemetry("ledger-server");

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://ledger_admin:ledger_secure_password_2026@localhost:5433/ledger_db".to_string()
    });

    info!("Connecting to PostgreSQL database...");
    let pool = PgPoolOptions::new()
        .max_connections(30)
        .connect(&database_url)
        .await?;

    info!("Applying schema migrations...");
    sqlx::migrate!("../../migrations").run(&pool).await?;

    let state = AppState {
        pool: Arc::new(pool),
    };

    let app = Router::new()
        .route("/v1/accounts", post(handle_create_account))
        .route("/v1/accounts/:id/balance", get(handle_get_account_balance))
        .route("/v1/transactions", post(handle_create_transaction))
        .route("/v1/audit/verify", get(handle_audit_verification))
        .route("/v1/reconciliation", post(handle_reconciliation))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Ledger Engine Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
