# ADR 0004: Payload-Hashed Idempotency Store

## Status
Accepted

## Context
Clients experience network drops and retry requests. Blindly rejecting existing idempotency keys causes failures, while blindly returning cached responses can mask critical client integration bugs where an existing key is accidentally reused with an altered payload (e.g., retrying a $50 transfer with a $500 transfer).

## Decision
The idempotency store computes and persists a SHA-256 fingerprint of the serialized request payload. When a duplicate key arrives:
1. Identical payload: Returns cached HTTP response code and body.
2. Altered payload: Aborts with a `409 Conflict` error and logs a payload mismatch.
3. In-flight transaction: Blocks duplicate execution via atomic row locking on the idempotency key.

## Consequences
- **Positive:** Eliminates double-entry risks while actively alerting upstream systems to key reuse misuse.
- **Negative:** Requires storing request hashes and response payloads in PostgreSQL.
