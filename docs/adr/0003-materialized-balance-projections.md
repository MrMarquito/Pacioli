# ADR 0003: Derived Materialized Balance Projections

## Status
Accepted

## Context
Calculating account balances dynamically by summing historical postings (`SELECT SUM(...) FROM postings WHERE account_id = $1`) degrades linearly ($\mathcal{O}(N)$) as transaction volume scales into millions of rows, violating sub-10ms latency SLAs for balance authorization checks.

## Decision
We maintain an `account_balances` table updated atomically within the same database transaction that appends journal entries. Immutable postings remain the single source of truth; the balance table functions as a strongly consistent materialized projection.

## Consequences
- **Positive:** Sub-millisecond $\mathcal{O}(1)$ account balance reads.
- **Positive:** In the event of projection drift or audit discovery, balances can be recomputed from genesis journal entries.
- **Negative:** Write transactions must update both the append-only postings table and the materialized balance row.
