# ADR 0002: Deterministic Row Locking over Serializable Retries

## Status
Accepted

## Context
Under standard relational database operation, concurrent transfers between the same accounts in opposite directions (Acc A $\to$ Acc B and Acc B $\to$ Acc A) produce unresolvable deadlocks (`SQLSTATE 40P01`). While `SERIALIZABLE` isolation resolves conflicts by aborting conflicting transactions, hot merchant accounts experience exponential transaction retry storms that collapse throughput under peak load.

## Decision
We enforce a strict global lock hierarchy under `READ COMMITTED` isolation. Before acquiring row-level locks, all involved account IDs are deduplicated and sorted lexicographically in ascending order. PostgreSQL locks are then acquired using `SELECT ... FOR UPDATE` in this fixed order.

## Consequences
- **Positive:** 100% elimination of deadlock aborts under high concurrency and hot-account Zipfian contention.
- **Positive:** Predictable lock queueing without exponential retry storms.
- **Negative:** Contended transactions serialize on the hot account, bounding peak throughput to single-row throughput limits for that specific account.
