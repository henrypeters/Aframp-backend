-- Create audit_replication_log table for tracking audit entry replication

CREATE TABLE IF NOT EXISTS audit_replication_log (
    entry_id UUID PRIMARY KEY,
    replicated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_replication_log_replicated ON audit_replication_log(replicated_at DESC);

COMMENT ON TABLE audit_replication_log IS 'Tracks which audit entries have been replicated to external systems';
