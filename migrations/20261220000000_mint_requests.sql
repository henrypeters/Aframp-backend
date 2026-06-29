-- Mint request approval workflow and SLA tracking schema.

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'escalation_action'
    ) THEN
        CREATE TYPE escalation_action AS ENUM (
            'sla_warning_sent',
            'sla_escalated',
            'sla_expired',
            'stellar_timebound_set',
            'stellar_timeout_failed'
        );
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION mint_set_updated_at_column()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$;

CREATE TABLE IF NOT EXISTS mint_requests (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    submitted_by        TEXT NOT NULL,
    destination_wallet  TEXT NOT NULL,
    amount_ngn          NUMERIC(36, 7) NOT NULL CHECK (amount_ngn > 0),
    amount_cngn         NUMERIC(36, 7) NOT NULL CHECK (amount_cngn > 0),
    rate_snapshot       NUMERIC(36, 7) NOT NULL CHECK (rate_snapshot > 0),
    approval_tier       SMALLINT NOT NULL CHECK (approval_tier BETWEEN 1 AND 3),
    required_approvals  SMALLINT NOT NULL CHECK (required_approvals BETWEEN 1 AND 3),
    status              TEXT NOT NULL DEFAULT 'pending_approval'
                            CHECK (status IN (
                                'pending_approval',
                                'partially_approved',
                                'approved',
                                'rejected',
                                'expired',
                                'executed'
                            )),
    reference           TEXT,
    metadata            JSONB NOT NULL DEFAULT '{}'::jsonb,
    expires_at          TIMESTAMPTZ NOT NULL DEFAULT (now() + INTERVAL '24 hours'),
    stellar_tx_hash     TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mint_requests_status
    ON mint_requests (status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mint_requests_destination_wallet
    ON mint_requests (destination_wallet, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mint_requests_reference
    ON mint_requests (reference);

DROP TRIGGER IF EXISTS trg_mint_requests_updated_at ON mint_requests;
CREATE TRIGGER trg_mint_requests_updated_at
    BEFORE UPDATE ON mint_requests
    FOR EACH ROW EXECUTE FUNCTION mint_set_updated_at_column();

CREATE TABLE IF NOT EXISTS mint_approvals (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    mint_request_id UUID NOT NULL REFERENCES mint_requests(id) ON DELETE CASCADE,
    approver_id     TEXT NOT NULL,
    approver_role   TEXT NOT NULL,
    action          TEXT NOT NULL CHECK (action IN ('approve', 'reject')),
    reason_code     TEXT,
    comment         TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mint_approvals_unique_actor
    ON mint_approvals (mint_request_id, approver_id);
CREATE INDEX IF NOT EXISTS idx_mint_approvals_request_created
    ON mint_approvals (mint_request_id, created_at);

CREATE TABLE IF NOT EXISTS mint_audit_log (
    id              BIGSERIAL PRIMARY KEY,
    mint_request_id UUID NOT NULL REFERENCES mint_requests(id) ON DELETE CASCADE,
    actor_id        TEXT NOT NULL,
    actor_role      TEXT,
    event_type      TEXT NOT NULL,
    from_status     TEXT,
    to_status       TEXT,
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mint_audit_log_request_created
    ON mint_audit_log (mint_request_id, created_at);

CREATE TABLE IF NOT EXISTS mint_sla_state (
    mint_request_id      UUID PRIMARY KEY REFERENCES mint_requests(id) ON DELETE CASCADE,
    stage                TEXT NOT NULL DEFAULT 'pending'
                             CHECK (stage IN ('pending', 'warned', 'escalated', 'expired', 'resolved')),
    warned_at            TIMESTAMPTZ,
    escalated_at         TIMESTAMPTZ,
    escalated_to         TEXT,
    expired_at           TIMESTAMPTZ,
    resolved_at          TIMESTAMPTZ,
    last_worker_run_id   UUID,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mint_sla_state_stage
    ON mint_sla_state (stage, updated_at DESC);

DROP TRIGGER IF EXISTS trg_mint_sla_state_updated_at ON mint_sla_state;
CREATE TRIGGER trg_mint_sla_state_updated_at
    BEFORE UPDATE ON mint_sla_state
    FOR EACH ROW EXECUTE FUNCTION mint_set_updated_at_column();

CREATE TABLE IF NOT EXISTS mint_escalation_log (
    id                BIGSERIAL PRIMARY KEY,
    mint_request_id   UUID NOT NULL REFERENCES mint_requests(id) ON DELETE CASCADE,
    action            escalation_action NOT NULL,
    elapsed_hours     DOUBLE PRECISION NOT NULL DEFAULT 0,
    notified_targets  JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata          JSONB NOT NULL DEFAULT '{}'::jsonb,
    worker_run_id     UUID,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mint_escalation_log_request_created
    ON mint_escalation_log (mint_request_id, created_at DESC);

CREATE TABLE IF NOT EXISTS mint_stellar_timebounds (
    mint_request_id       UUID PRIMARY KEY REFERENCES mint_requests(id) ON DELETE CASCADE,
    min_time_unix         BIGINT NOT NULL,
    max_time_unix         BIGINT NOT NULL,
    sla_expires_at        TIMESTAMPTZ NOT NULL,
    is_timeout_failed     BOOLEAN NOT NULL DEFAULT FALSE,
    timeout_detected_at   TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mint_stellar_timebounds_timeout_scan
    ON mint_stellar_timebounds (is_timeout_failed, max_time_unix);

DROP TRIGGER IF EXISTS trg_mint_stellar_timebounds_updated_at ON mint_stellar_timebounds;
CREATE TRIGGER trg_mint_stellar_timebounds_updated_at
    BEFORE UPDATE ON mint_stellar_timebounds
    FOR EACH ROW EXECUTE FUNCTION mint_set_updated_at_column();

CREATE OR REPLACE FUNCTION mint_requests_init_sla_state()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO mint_sla_state (mint_request_id, stage)
    VALUES (NEW.id, 'pending')
    ON CONFLICT (mint_request_id) DO NOTHING;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_mint_requests_init_sla_state ON mint_requests;
CREATE TRIGGER trg_mint_requests_init_sla_state
    AFTER INSERT ON mint_requests
    FOR EACH ROW EXECUTE FUNCTION mint_requests_init_sla_state();

INSERT INTO mint_sla_state (mint_request_id, stage)
SELECT id, 'pending'
FROM mint_requests
ON CONFLICT (mint_request_id) DO NOTHING;
