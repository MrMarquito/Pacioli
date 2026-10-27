-- Enable cryptographic extensions for UUID generation if needed
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- 1. Accounts Table: Chart of accounts
CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    currency VARCHAR(3) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'FROZEN', 'CLOSED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 2. Journal Entries: Immutable transaction envelopes
CREATE TABLE journal_entries (
    seq BIGINT GENERATED ALWAYS AS IDENTITY,
    id UUID PRIMARY KEY,
    idempotency_key VARCHAR(255) NOT NULL UNIQUE,
    description TEXT NOT NULL,
    currency VARCHAR(3) NOT NULL,
    posted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    previous_entry_hash BYTEA,
    entry_hash BYTEA NOT NULL
);

-- 3. Postings: Individual balance adjustments belonging to an entry
CREATE TABLE postings (
    id UUID PRIMARY KEY,
    entry_id UUID NOT NULL REFERENCES journal_entries(id) ON DELETE RESTRICT,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    amount NUMERIC(28, 4) NOT NULL CHECK (amount > 0),
    direction VARCHAR(6) NOT NULL CHECK (direction IN ('DEBIT', 'CREDIT')),
    sequence_num INT NOT NULL CHECK (sequence_num >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (entry_id, sequence_num)
);

-- 4. Derived Balances: Materialized projection for sub-millisecond lookups
CREATE TABLE account_balances (
    account_id UUID PRIMARY KEY REFERENCES accounts(id) ON DELETE RESTRICT,
    balance NUMERIC(28, 4) NOT NULL DEFAULT 0.0000,
    currency VARCHAR(3) NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    last_entry_id UUID REFERENCES journal_entries(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 5. Idempotency Store: Payload hash tracking
CREATE TABLE idempotency_keys (
    key VARCHAR(255) PRIMARY KEY,
    payload_hash BYTEA NOT NULL,
    response_code INT,
    response_body JSONB,
    status VARCHAR(20) NOT NULL CHECK (status IN ('PROCESSING', 'COMMITTED', 'REJECTED')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 6. Transactional Outbox: Event relay queue
CREATE TABLE outbox_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    aggregate_type VARCHAR(50) NOT NULL,
    aggregate_id UUID NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ
);

-- 7. High-Performance Query Indexes
CREATE INDEX idx_journal_entries_seq ON journal_entries (seq ASC);
CREATE INDEX idx_postings_account_created ON postings (account_id, created_at DESC);
CREATE INDEX idx_journal_entries_posted_at ON journal_entries (posted_at DESC);
CREATE INDEX idx_outbox_unprocessed ON outbox_events (created_at ASC) WHERE processed_at IS NULL;

-- 8. Immutability Enforcement Triggers
CREATE OR REPLACE FUNCTION reject_ledger_mutation()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'CRITICAL INVARIANT VIOLATION: % operations are prohibited on table %', TG_OP, TG_TABLE_NAME;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_deny_journal_entries_mutation
BEFORE UPDATE OR DELETE ON journal_entries
FOR EACH ROW EXECUTE FUNCTION reject_ledger_mutation();

CREATE TRIGGER trg_deny_postings_mutation
BEFORE UPDATE OR DELETE ON postings
FOR EACH ROW EXECUTE FUNCTION reject_ledger_mutation();
