# ADR 0005: SHA-256 Cryptographic Audit Chaining

## Status
Accepted

## Context
Financial compliance frameworks require tamper-evident guarantees against administrative tampering. Database superusers can alter rows directly, bypassing application-level checks.

## Decision
Each journal entry computes a cryptographic SHA-256 digest:
$$H_n = \text{SHA-256}(H_{n-1} \parallel \text{EntryID} \parallel \text{Currency} \parallel \text{NormalizedPostings})$$
The previous entry's hash is stored in `previous_entry_hash`. An automated audit crawler walks the entire chain from genesis to mathematically assert ledger continuity.

## Consequences
- **Positive:** Any out-of-band row mutation, deletion, or insertion breaks the hash chain and pinpoints the corrupted entry.
- **Negative:** Writing journal entries requires an advisory lock to maintain chain linearity, adding small serialization overhead during write commits.
