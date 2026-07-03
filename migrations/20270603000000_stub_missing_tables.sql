-- Stub migration: create all tables referenced in code that don't exist yet
-- These are minimal stubs to allow sqlx to compile; they can be filled out later.

-- ── KYA (Know Your Agent) tables ─────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kya_reputation_scores (
    id                        UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did                 TEXT        NOT NULL,
    domain                    TEXT        NOT NULL,
    score                     FLOAT8      NOT NULL DEFAULT 50.0,
    total_interactions        BIGINT      NOT NULL DEFAULT 0,
    successful_interactions   BIGINT      NOT NULL DEFAULT 0,
    failed_interactions       BIGINT      NOT NULL DEFAULT 0,
    last_updated              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (agent_did, domain)
);

CREATE TABLE IF NOT EXISTS kya_agent_identities (
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    did               TEXT        NOT NULL UNIQUE,
    method            TEXT        NOT NULL,
    network           TEXT        NOT NULL,
    identifier        TEXT        NOT NULL,
    name              TEXT        NOT NULL,
    description       TEXT,
    owner_address     TEXT        NOT NULL,
    public_key        TEXT        NOT NULL,
    capabilities      JSONB       NOT NULL DEFAULT '[]',
    service_endpoints JSONB       NOT NULL DEFAULT '[]',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS kya_attestations (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did    TEXT        NOT NULL,
    issuer_did   TEXT        NOT NULL,
    domain       TEXT        NOT NULL,
    claim        TEXT        NOT NULL,
    evidence_uri TEXT,
    signature    TEXT        NOT NULL,
    issued_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at   TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS kya_competence_proofs (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did   TEXT        NOT NULL,
    domain      TEXT        NOT NULL,
    proof_data  JSONB       NOT NULL DEFAULT '{}',
    verified    BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS kya_feedback_tokens (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did       TEXT        NOT NULL,
    client_did      TEXT        NOT NULL,
    interaction_id  UUID        NOT NULL,
    domain          TEXT        NOT NULL,
    authorized_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    used            BOOLEAN     NOT NULL DEFAULT FALSE,
    signature       TEXT        NOT NULL
);

CREATE TABLE IF NOT EXISTS kya_cross_platform_reputation (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_did     TEXT        NOT NULL,
    platform      TEXT        NOT NULL,
    external_score FLOAT8,
    synced_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (agent_did, platform)
);

-- ── SLA tables ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS sla_policies (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    corridor_id  TEXT        NOT NULL,
    metric       TEXT        NOT NULL,
    threshold_ms FLOAT8      NOT NULL,
    enabled      BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (corridor_id, metric)
);

CREATE TABLE IF NOT EXISTS sla_breach_events (
    id           UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id    UUID           REFERENCES sla_policies(id),
    corridor_id  TEXT           NOT NULL,
    observed_ms  NUMERIC(18,4)  NOT NULL,
    threshold_ms NUMERIC(18,4)  NOT NULL,
    created_at   TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS sla_breach_audit_overrides (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    breach_event_id  UUID        NOT NULL,
    root_cause       TEXT        NOT NULL,
    actor            TEXT        NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS sla_partner_webhooks (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    corridor_id  TEXT        NOT NULL,
    url          TEXT        NOT NULL,
    enabled      BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Abuse detection tables ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS abuse_cases (
    id                   UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    consumer_ids         UUID[]      NOT NULL DEFAULT '{}',
    detection_signals    JSONB       NOT NULL DEFAULT '{}',
    composite_confidence FLOAT8      NOT NULL DEFAULT 0.0,
    response_tier        TEXT        NOT NULL DEFAULT 'monitor',
    status               TEXT        NOT NULL DEFAULT 'open',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at          TIMESTAMPTZ,
    resolution_notes     TEXT,
    escalated_by         TEXT,
    resolved_by          TEXT,
    false_positive       BOOLEAN,
    whitelisted_signals  TEXT[]      NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS abuse_response_actions (
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tier              TEXT        NOT NULL,
    consumer_ids      UUID[]      NOT NULL DEFAULT '{}',
    applied_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at        TIMESTAMPTZ,
    reason            TEXT        NOT NULL,
    evidence_case_id  UUID        REFERENCES abuse_cases(id),
    actions_taken     TEXT[]      NOT NULL DEFAULT '{}',
    notification_sent BOOLEAN     NOT NULL DEFAULT FALSE
);

-- ── Payment corridor tables ───────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS payment_corridors (
    id                    UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    source_country        TEXT        NOT NULL,
    destination_country   TEXT        NOT NULL,
    source_currency       TEXT        NOT NULL,
    destination_currency  TEXT        NOT NULL,
    status                TEXT        NOT NULL DEFAULT 'open',
    status_reason         TEXT,
    min_transfer_amount   NUMERIC(20,8) NOT NULL DEFAULT 0,
    max_transfer_amount   NUMERIC(20,8) NOT NULL DEFAULT 999999999,
    delivery_methods      TEXT[]      NOT NULL DEFAULT '{}',
    bridge_asset          TEXT,
    risk_score            SMALLINT    NOT NULL DEFAULT 50,
    required_kyc_tier     TEXT        NOT NULL DEFAULT 'basic',
    display_name          TEXT,
    estimated_minutes     INT,
    is_featured           BOOLEAN     NOT NULL DEFAULT FALSE,
    config                JSONB       NOT NULL DEFAULT '{}',
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by            UUID
);

CREATE TABLE IF NOT EXISTS corridor_route_hops (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    corridor_id  UUID        NOT NULL REFERENCES payment_corridors(id) ON DELETE CASCADE,
    hop_order    SMALLINT    NOT NULL,
    from_asset   TEXT        NOT NULL,
    to_asset     TEXT        NOT NULL,
    provider     TEXT,
    is_active    BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (corridor_id, hop_order)
);

CREATE TABLE IF NOT EXISTS corridor_health (
    corridor_id     UUID           NOT NULL REFERENCES payment_corridors(id) ON DELETE CASCADE,
    bucket_start    TIMESTAMPTZ    NOT NULL,
    total_attempts  INT            NOT NULL DEFAULT 0,
    successful      INT            NOT NULL DEFAULT 0,
    failed          INT            NOT NULL DEFAULT 0,
    total_volume    NUMERIC(20,8)  NOT NULL DEFAULT 0,
    avg_latency_ms  INT,
    updated_at      TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    PRIMARY KEY (corridor_id, bucket_start)
);

CREATE TABLE IF NOT EXISTS corridor_audit_log (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    corridor_id     UUID        NOT NULL,
    action          TEXT        NOT NULL,
    changed_by      UUID,
    changed_by_role TEXT,
    previous_value  JSONB,
    new_value       JSONB,
    reason          TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── DeFi / Treasury tables ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS treasury_allocations (
    allocation_id         UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
    protocol_id           TEXT          NOT NULL,
    allocation_type       TEXT          NOT NULL,
    allocated_amount      NUMERIC(30,8) NOT NULL,
    current_value         NUMERIC(30,8) NOT NULL,
    yield_earned          NUMERIC(30,8) NOT NULL DEFAULT 0,
    allocation_percentage FLOAT8        NOT NULL DEFAULT 0,
    allocated_at          TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    last_updated_at       TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    status                TEXT          NOT NULL DEFAULT 'active'
);

CREATE TABLE IF NOT EXISTS defi_strategy_allocations (
    id            UUID          PRIMARY KEY DEFAULT gen_random_uuid(),
    strategy_id   UUID          NOT NULL,
    protocol_id   TEXT          NOT NULL,
    amount        NUMERIC(30,8) NOT NULL,
    allocated_at  TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

-- yield_accrual_entries referenced in analytics/repository.rs; actual table is yield_accrual_records
-- create a view alias so both names work
CREATE OR REPLACE VIEW yield_accrual_entries AS SELECT * FROM yield_accrual_records;

CREATE TABLE IF NOT EXISTS risk_disclosure_acceptances (
    acceptance_id       UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id             TEXT        NOT NULL,
    product_id          UUID        NOT NULL,
    disclosure_version  TEXT        NOT NULL DEFAULT '1.0',
    accepted_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address          TEXT,
    UNIQUE (user_id, product_id)
);

-- ── Missing columns on existing tables ───────────────────────────────────────

-- cngn_savings_accounts: add updated_at
ALTER TABLE cngn_savings_accounts
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- reconciliation_reports: add notes and generated_at
ALTER TABLE reconciliation_reports
    ADD COLUMN IF NOT EXISTS notes TEXT;
ALTER TABLE reconciliation_reports
    ADD COLUMN IF NOT EXISTS generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- discrepancy_log: add notes column (code queries "notes" but column is "resolution_notes")
ALTER TABLE discrepancy_log
    ADD COLUMN IF NOT EXISTS notes TEXT GENERATED ALWAYS AS (resolution_notes) STORED;

-- bug_bounty_reports: add columns that pentest code expects
ALTER TABLE bug_bounty_reports
    ADD COLUMN IF NOT EXISTS reporter_contact TEXT;
ALTER TABLE bug_bounty_reports
    ADD COLUMN IF NOT EXISTS reporter_name TEXT;
ALTER TABLE bug_bounty_reports
    ADD COLUMN IF NOT EXISTS triage_notes TEXT;
ALTER TABLE bug_bounty_reports
    ADD COLUMN IF NOT EXISTS reward_amount NUMERIC(20,8);
ALTER TABLE bug_bounty_reports
    ADD COLUMN IF NOT EXISTS reward_currency TEXT NOT NULL DEFAULT 'USDC';
ALTER TABLE bug_bounty_reports
    ADD COLUMN IF NOT EXISTS remediation_timeline_provided_at TIMESTAMPTZ;
