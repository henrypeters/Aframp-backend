-- Migration: Cache Invalidation Audit Log (Issue #459)
--
-- Maintainer action required before merge/QA:
--   1. sqlx migrate run
--   2. cargo test --test cache_integration_tests -- --ignored  (requires REDIS_URL)
-- Rollback: DROP TABLE cache_invalidation_logs;

CREATE TABLE IF NOT EXISTS cache_invalidation_logs (
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    initiator_id      UUID,
    initiator_role    TEXT,
    target_namespace  TEXT        NOT NULL,
    pattern_used      TEXT        NOT NULL,
    keys_deleted      BIGINT      NOT NULL DEFAULT 0,
    reason            TEXT,
    triggered_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_cache_inv_namespace ON cache_invalidation_logs (target_namespace);
CREATE INDEX idx_cache_inv_time      ON cache_invalidation_logs (triggered_at DESC);
