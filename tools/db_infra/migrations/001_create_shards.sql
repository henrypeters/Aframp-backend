-- Example migration to create partitioned ledger table per shard
BEGIN;

CREATE TABLE IF NOT EXISTS ledger (
    id BIGSERIAL PRIMARY KEY,
    account_id UUID NOT NULL,
    corridor_id TEXT NOT NULL,
    amount BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
) PARTITION BY HASH (account_id);

-- This migration only defines the parent; create partitions for known shard ids later.
COMMIT;
