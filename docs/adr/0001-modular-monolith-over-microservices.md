# ADR 0001: Modular Monolith over Microservices

## Status
Accepted

## Context
Payment and financial ledger platforms must enforce strict transactional invariants: money cannot be created or destroyed, postings must balance ($D - C = 0$), and materialized balances must reflect committed postings. In microservice architectures, cross-service consistency requires two-phase commit (2PC) or Saga patterns, which introduce eventual consistency windows, distributed failure modes, and operational overhead.

## Decision
We implement the core ledger engine as a modular monolith compiled into a single deployable unit backed by a single PostgreSQL instance. Domain seams (`ledger-accounts`, `ledger-journal`, `ledger-balances`, `ledger-outbox`, `ledger-audit`, `ledger-reconciliation`) communicate exclusively via public Rust trait interfaces and in-process calls. Module boundaries are enforced via Cargo workspaces and automated architecture fitness tests in CI (`cargo test -p architecture_fitness`).

## Consequences
- **Positive:** ACID guarantees across Journal, Balances, Outbox, and Idempotency in a single database transaction.
- **Positive:** Zero distributed consensus latency or network partition failures between core modules.
- **Negative:** Horizontal scaling is tied to a single database cluster; vertical scaling and connection pooling must be managed carefully under heavy workloads.
