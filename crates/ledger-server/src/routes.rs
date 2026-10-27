use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use ledger_accounts::create_account;
use ledger_audit::verify_ledger_cryptographic_chain;
use ledger_core::{AccountId, Currency, Direction, JournalEntryDraft, PostingDraft};
use ledger_journal::{execute_journal_entry, CommitOutcome};
use ledger_reconciliation::{reconcile_account, ExternalAccountStatement};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use tracing::instrument;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<sqlx::PgPool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAccountRequest {
    pub id: Option<Uuid>,
    pub currency: Currency,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostingRequest {
    pub account_id: Uuid,
    pub direction: Direction,
    pub amount: Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTransactionRequest {
    pub description: String,
    pub currency: Currency,
    pub postings: Vec<PostingRequest>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[instrument(name = "http.create_account", skip(state))]
pub async fn handle_create_account(
    State(state): State<AppState>,
    Json(payload): Json<CreateAccountRequest>,
) -> impl IntoResponse {
    let requested_id = payload.id.map(AccountId);
    match create_account(&state.pool, requested_id, payload.currency).await {
        Ok(account) => (
            StatusCode::CREATED,
            Json(ApiResponse {
                success: true,
                data: Some(account),
                error: None,
            }),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}

#[instrument(name = "http.get_balance", skip(state))]
pub async fn handle_get_account_balance(
    State(state): State<AppState>,
    Path(account_id): Path<Uuid>,
) -> impl IntoResponse {
    let row = sqlx::query(
        r#"
        SELECT balance, currency, version, updated_at
        FROM account_balances
        WHERE account_id = $1
        "#,
    )
    .bind(account_id)
    .fetch_optional(&*state.pool)
    .await;

    match row {
        Ok(Some(r)) => {
            let balance: Decimal = r.get("balance");
            let currency: String = r.get("currency");
            let version: i64 = r.get("version");
            (
                StatusCode::OK,
                Json(ApiResponse {
                    success: true,
                    data: Some(serde_json::json!({
                        "account_id": account_id,
                        "balance": balance,
                        "currency": currency,
                        "version": version
                    })),
                    error: None,
                }),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(format!("Account {} not found", account_id)),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
            }),
        ),
    }
}

#[instrument(name = "http.create_transaction", skip(state, headers, payload))]
pub async fn handle_create_transaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateTransactionRequest>,
) -> impl IntoResponse {
    let idempotency_key = match headers.get("Idempotency-Key") {
        Some(k) => match k.to_str() {
            Ok(v) if !v.trim().is_empty() => v.to_string(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse {
                        success: false,
                        data: None,
                        error: Some("Invalid Idempotency-Key header value".into()),
                    }),
                );
            }
        },
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some("Missing required Idempotency-Key header".into()),
                }),
            );
        }
    };

    let mut drafts = Vec::new();
    for p in &payload.postings {
        match PostingDraft::new(AccountId(p.account_id), p.direction, p.amount) {
            Ok(d) => drafts.push(d),
            Err(e) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(ApiResponse {
                        success: false,
                        data: None,
                        error: Some(e.to_string()),
                    }),
                );
            }
        }
    }

    let draft = match JournalEntryDraft::new(
        idempotency_key,
        payload.description.clone(),
        payload.currency,
        drafts,
    ) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                }),
            );
        }
    };

    // Canonical fingerprint hashes the incoming request payload from the client
    let raw_payload_fingerprint = serde_json::to_vec(&payload).unwrap_or_default();

    match execute_journal_entry(&state.pool, draft, &raw_payload_fingerprint).await {
        Ok(CommitOutcome::Committed(entry_id)) => (
            StatusCode::CREATED,
            Json(ApiResponse {
                success: true,
                data: Some(serde_json::json!({ "entry_id": entry_id, "status": "COMMITTED" })),
                error: None,
            }),
        ),
        Ok(CommitOutcome::AlreadyProcessed(cached)) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                data: Some(cached),
                error: None,
            }),
        ),
        Err(err) => (
            StatusCode::CONFLICT,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}

#[instrument(name = "http.audit_verify", skip(state))]
pub async fn handle_audit_verification(State(state): State<AppState>) -> impl IntoResponse {
    match verify_ledger_cryptographic_chain(&state.pool).await {
        Ok(report) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                data: Some(serde_json::json!({
                    "entries_verified": report.entries_verified,
                    "is_valid": report.is_valid
                })),
                error: None,
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}

#[instrument(name = "http.reconcile", skip(state))]
pub async fn handle_reconciliation(
    State(state): State<AppState>,
    Json(stmt): Json<ExternalAccountStatement>,
) -> impl IntoResponse {
    match reconcile_account(&state.pool, &stmt).await {
        Ok(report) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                data: Some(serde_json::to_value(report).unwrap()),
                error: None,
            }),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                success: false,
                data: None,
                error: Some(err.to_string()),
            }),
        ),
    }
}
