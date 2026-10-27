# Pacioli: General-Purpose Double-Entry Accounting Engine

[![CI Quality Gate](https://github.com/MrMarquito/Pacioli/actions/workflows/ci.yml/badge.svg)](https://github.com/MrMarquito/Pacioli/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/rust-1.78%2B-orange.svg)](https://www.rust-lang.org)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-336791.svg)](https://www.postgresql.org)

A high-integrity double-entry financial ledger engine built in Rust and PostgreSQL. Designed for payment processors, fintech platforms, and marketplace backends requiring mathematical zero-loss guarantees, deadlock-free concurrency, and cryptographic tamper resistance.

## Table of Contents

- [Core Guarantees & Invariants](#core-guarantees--invariants)
- [Architecture](#architecture)
- [Benchmark & Chaos Results](#benchmark--chaos-results)
- [Quickstart](#quickstart)
- [API Reference](#api-reference)

---

## Core Guarantees & Invariants

1. **Strict Double-Entry Invariant:** Every entry must balance to zero ($\sum \text{Debits} - \sum \text{Credits} = 0$). Single-legged or unbalanced transactions are rejected at both domain and SQL layers.
2. **Fixed-Point Decimal Math:** Eliminates binary floating-point rounding inaccuracies using 128-bit fixed-point decimal arithmetic (`rust_decimal`).
3. **Database-Level Immutability:** Triggers intercept and abort all `UPDATE` and `DELETE` operations on `journal_entries` and `postings` tables. Financial adjustments require offsetting reversing entries.
4. **Deadlock Immunity via Deterministic Locking:** Account locks are sorted lexicographically prior to acquisition, eliminating circular wait deadlocks under high concurrency.
5. **Payload-Hashed Idempotency:** Prevents duplicate financial transfers while detecting and rejecting same-key/different-payload integration bugs.
6. **Cryptographic Audit Chaining:** Every journal record is linked to its predecessor via SHA-256 hashes ($H_n = \text{SHA-256}(H_{n-1} \parallel \text{Data})$), enabling end-to-end verification.
7. **Transactional Outbox:** Financial postings and downstream notification events commit within the same ACID boundary, preventing phantom events and lost updates.

---

## Architecture

Modular monolith with strict boundary enforcement:

```text
crates/
├── ledger-core/            # Pure domain models (Money, Currency, Postings, Hashes)
├── ledger-accounts/        # Chart of accounts and lifecycle management
├── ledger-journal/         # ONLY crate permitted to post transactions
├── ledger-balances/        # Materialized balance projection
├── ledger-idempotency/     # Payload hashing and atomic key reservations
├── ledger-outbox/          # Transactional event relay
├── ledger-reconciliation/  # External bank statement comparison engine
├── ledger-audit/           # SHA-256 cryptographic chain auditor
└── ledger-server/          # Axum REST API with OpenTelemetry instrumentation
```

Architecture boundaries are asserted via compile-time dependency trees and automated fitness tests:

```bash
cargo test -p architecture_fitness
```

---

## Benchmark & Chaos Results

Tested on native ARM64 / x86_64 architecture against containerized PostgreSQL 16.

### 1. Hot-Account Contention Benchmark

**Workload:** 100 concurrent asynchronous transfers contending on the exact same merchant account.

**Concurrency mechanism:** Deterministic ascending row locking (`SELECT ... FOR UPDATE`).

| Metric                  | Result          |
| ----------------------- | --------------- |
| Total Transactions      | 100             |
| Elapsed Time            | 3.95 seconds    |
| Throughput (TPS)        | 25.28 tx/sec    |
| p50 Latency             | 3,343 ms        |
| p99 Latency             | 3,945 ms        |
| Deadlock Aborts (40P01) | 0 (0.00%)       |

**Result analysis:** Despite extreme Zipfian contention where all 100 workers locked the exact same balance row, deterministic lock acquisition completely eliminated deadlocks, allowing all 100 transactions to commit without retry loops.

### 2. Chaos Injection Scenarios

Located in `tests/benchmark_chaos`.

- **Mid-flight worker crash:** An in-flight transaction locked account rows and modified balance tables before encountering an uncommitted process abort. PostgreSQL rolled back the transaction; verified zero balance leakage and preserved cryptographic chain integrity.
- **Tamper detection:** Injected unauthorized database mutations were flagged by `ledger-audit` with pinpointed `ChainBroken` error reports.

---

## Quickstart

### Prerequisites

- Rust 1.78+
- Docker & Docker Compose

### 1. Start the database

```bash
docker compose up -d
```

### 2. Run the test suite

```bash
# Architecture boundary fitness test
cargo test -p architecture_fitness

# Database trigger & immutability test
cargo test -p db_tests -- --nocapture

# Core transaction engine concurrency & idempotency test
cargo test -p engine_tests -- --nocapture

# Audit chain, outbox relay, and external reconciliation test
cargo test -p verification_tests -- --nocapture

# Stress benchmarks and chaos injection test
cargo test -p benchmark_chaos -- --nocapture
```

### 3. Start the API server

```bash
cargo run -p ledger-server
```

The server binds to `http://0.0.0.0:8080` with OpenTelemetry tracing enabled.

---

## API Reference

Base URL: `http://localhost:8080`

| Method | Endpoint                    | Description                          |
| ------ | --------------------------- | ------------------------------------ |
| POST   | `/v1/accounts`              | Create an account                    |
| POST   | `/v1/transactions`          | Submit a double-entry transaction    |
| GET    | `/v1/accounts/:id/balance`  | Query an account balance             |
| GET    | `/v1/audit/verify`          | Verify the cryptographic audit chain |
| POST   | `/v1/reconciliation`        | Reconcile against an external statement |

### Create Account

`POST /v1/accounts`

```json
{
  "currency": "USD"
}
```

### Submit Double-Entry Transaction

`POST /v1/transactions`

**Headers:** `Idempotency-Key: <UUID>`

```json
{
  "description": "Merchant settlement",
  "currency": "USD",
  "postings": [
    { "account_id": "0192e22c-5b23-7a91-953e-b7d14213d2f2", "direction": "Debit", "amount": 150.00 },
    { "account_id": "0192e22c-5b23-7d12-8821-ca41b2e3a199", "direction": "Credit", "amount": 150.00 }
  ]
}
```

### Query Account Balance

`GET /v1/accounts/:id/balance`

### Verify Cryptographic Audit Chain

`GET /v1/audit/verify`

### Reconcile Against External Statement

`POST /v1/reconciliation`

```json
{
  "account_id": "0192e22c-5b23-7a91-953e-b7d14213d2f2",
  "external_balance": 150.00,
  "statement_date": "2026-10-27T12:00:00Z"
}
```
