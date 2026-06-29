-- #487 Smart Order Routing & Treasury Rebalancing Schemas
-- Tables: liquidity_venues, smart_order_executions, treasury_rebalancing_rules

-- ── ENUMs (idempotent) ────────────────────────────────────────────────────────
DO $$ BEGIN
    CREATE TYPE venue_type AS ENUM ('regional_bank', 'stellar_amm', 'mto', 'cex', 'dex');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE venue_status AS ENUM ('active', 'degraded', 'offline', 'suspended');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE sor_status AS ENUM ('pending', 'routing', 'partial', 'completed', 'failed', 'rolled_back');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE child_order_status AS ENUM ('pending', 'submitted', 'filled', 'partial_fill', 'failed', 'timed_out');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE rebalancing_trigger AS ENUM ('threshold_breach', 'scheduled', 'manual');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
    CREATE TYPE rebalance_status AS ENUM ('initiated', 'in_progress', 'completed', 'failed');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ── Liquidity venues (add missing columns to existing table) ──────────────────
CREATE TABLE IF NOT EXISTS liquidity_venues (
    venue_id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                VARCHAR(255)   NOT NULL,
    venue_type          TEXT           NOT NULL DEFAULT 'cex',
    status              TEXT           NOT NULL DEFAULT 'active',
    api_endpoint        TEXT           NOT NULL DEFAULT '',
    api_credentials     BYTEA,
    supported_currencies TEXT[]        NOT NULL DEFAULT '{}',
    daily_volume_limit  NUMERIC(28,7)  NOT NULL DEFAULT 0,
    used_volume_today   NUMERIC(28,7)  NOT NULL DEFAULT 0,
    execution_fee_bps   NUMERIC(10,4)  NOT NULL DEFAULT 0,
    spread_bps          NUMERIC(10,4)  NOT NULL DEFAULT 0,
    last_heartbeat_at   TIMESTAMPTZ,
    created_at          TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);

ALTER TABLE liquidity_venues
    ADD COLUMN IF NOT EXISTS api_endpoint        TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS api_credentials     BYTEA,
    ADD COLUMN IF NOT EXISTS supported_currencies TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS used_volume_today   NUMERIC(28,7) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS execution_fee_bps   NUMERIC(10,4) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS spread_bps          NUMERIC(10,4) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_heartbeat_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS venue_type_v2       TEXT NOT NULL DEFAULT 'cex',
    ADD COLUMN IF NOT EXISTS status_v2           TEXT NOT NULL DEFAULT 'active';

CREATE INDEX IF NOT EXISTS idx_venues_type ON liquidity_venues(venue_type_v2);

CREATE INDEX IF NOT EXISTS idx_venues_status ON liquidity_venues(connection_status);
-- venue_type column added by ALTER below, index created after

-- ── Smart order executions ────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS smart_order_executions (
    execution_id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_transaction_id UUID NOT NULL,
    correlation_tag       VARCHAR(64)   NOT NULL DEFAULT '',
    source_currency       VARCHAR(10)   NOT NULL DEFAULT '',
    target_currency       VARCHAR(10)   NOT NULL DEFAULT '',
    total_amount          NUMERIC(28,7) NOT NULL DEFAULT 0,
    status                TEXT          NOT NULL DEFAULT 'pending',
    routing_plan          JSONB         NOT NULL DEFAULT '[]',
    realized_slippage_bps NUMERIC(10,4),
    path_calc_ms          INTEGER,
    created_at            TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    completed_at          TIMESTAMPTZ
);

ALTER TABLE smart_order_executions
    ADD COLUMN IF NOT EXISTS correlation_tag       VARCHAR(64)   NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS source_currency       VARCHAR(10)   NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS target_currency       VARCHAR(10)   NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS routing_plan          JSONB         NOT NULL DEFAULT '[]',
    ADD COLUMN IF NOT EXISTS realized_slippage_bps NUMERIC(10,4),
    ADD COLUMN IF NOT EXISTS path_calc_ms          INTEGER,
    ADD COLUMN IF NOT EXISTS completed_at          TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_sor_parent_tx   ON smart_order_executions(primary_transaction_id);
CREATE INDEX IF NOT EXISTS idx_sor_correlation ON smart_order_executions(correlation_tag) WHERE correlation_tag IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sor_status      ON smart_order_executions(status);

-- ── SOR child orders ──────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS sor_child_orders (
    child_order_id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id     UUID          NOT NULL REFERENCES smart_order_executions(execution_id),
    venue_id         UUID          NOT NULL REFERENCES liquidity_venues(venue_id),
    allocation_pct   NUMERIC(6,4)  NOT NULL DEFAULT 0,
    allocated_amount NUMERIC(28,7) NOT NULL DEFAULT 0,
    filled_amount    NUMERIC(28,7) NOT NULL DEFAULT 0,
    status           TEXT          NOT NULL DEFAULT 'pending',
    venue_order_ref  VARCHAR(128),
    slippage_bps     NUMERIC(10,4),
    submitted_at     TIMESTAMPTZ,
    filled_at        TIMESTAMPTZ,
    failed_reason    TEXT
);

CREATE INDEX IF NOT EXISTS idx_child_execution ON sor_child_orders(execution_id);
CREATE INDEX IF NOT EXISTS idx_child_venue      ON sor_child_orders(venue_id);

-- ── Treasury rebalancing rules ────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS treasury_rebalancing_rules (
    rule_id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    currency            VARCHAR(10)   NOT NULL DEFAULT '' UNIQUE,
    currency_code        VARCHAR(10)   NOT NULL DEFAULT '',
    min_inventory_pct    NUMERIC(6,4)  NOT NULL DEFAULT 0.10,
    target_inventory_pct NUMERIC(6,4)  NOT NULL DEFAULT 0.20,
    max_inventory_pct    NUMERIC(6,4)  NOT NULL DEFAULT 0.50,
    target_percentage    NUMERIC(6,4)  NOT NULL DEFAULT 0.20,
    min_threshold_percentage NUMERIC(6,4) NOT NULL DEFAULT 0.10,
    trigger_type         TEXT          NOT NULL DEFAULT 'threshold_breach',
    schedule_cron        VARCHAR(64),
    enabled              BOOLEAN       NOT NULL DEFAULT TRUE,
    last_triggered_at    TIMESTAMPTZ,
    created_at           TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

ALTER TABLE treasury_rebalancing_rules
    ADD COLUMN IF NOT EXISTS currency            VARCHAR(10) NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS target_percentage   NUMERIC(6,4) NOT NULL DEFAULT 0.20,
    ADD COLUMN IF NOT EXISTS min_threshold_percentage NUMERIC(6,4) NOT NULL DEFAULT 0.10,
    ADD COLUMN IF NOT EXISTS min_inventory_pct   NUMERIC(6,4) NOT NULL DEFAULT 0.10,
    ADD COLUMN IF NOT EXISTS target_inventory_pct NUMERIC(6,4) NOT NULL DEFAULT 0.20,
    ADD COLUMN IF NOT EXISTS max_inventory_pct   NUMERIC(6,4) NOT NULL DEFAULT 0.50,
    ADD COLUMN IF NOT EXISTS trigger_type        TEXT         NOT NULL DEFAULT 'threshold_breach',
    ADD COLUMN IF NOT EXISTS schedule_cron       VARCHAR(64),
    ADD COLUMN IF NOT EXISTS enabled             BOOLEAN      NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS last_triggered_at   TIMESTAMPTZ;

-- ── Treasury rebalancing log ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS treasury_rebalancing_log (
    log_id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id            UUID          NOT NULL REFERENCES treasury_rebalancing_rules(rule_id),
    currency_code      VARCHAR(10)   NOT NULL DEFAULT '',
    trigger_type       TEXT          NOT NULL DEFAULT 'threshold_breach',
    amount_rebalanced  NUMERIC(28,7) NOT NULL DEFAULT 0,
    status             TEXT          NOT NULL DEFAULT 'initiated',
    stellar_tx_hash    VARCHAR(128),
    redis_lock_key     VARCHAR(128),
    error_message      TEXT,
    initiated_at       TIMESTAMPTZ   NOT NULL DEFAULT NOW(),
    completed_at       TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_rebalance_log_rule     ON treasury_rebalancing_log(rule_id);
CREATE INDEX IF NOT EXISTS idx_rebalance_log_currency ON treasury_rebalancing_log(currency_code);
CREATE INDEX IF NOT EXISTS idx_rebalance_log_status   ON treasury_rebalancing_log(status);

-- Seed default rebalancing rules
INSERT INTO treasury_rebalancing_rules
    (rule_id, currency, min_inventory_pct, target_inventory_pct, max_inventory_pct, target_percentage, min_threshold_percentage)
VALUES
    (gen_random_uuid(), 'NGN',  0.20, 0.30, 0.50, 0.30, 0.20),
    (gen_random_uuid(), 'KES',  0.15, 0.25, 0.45, 0.25, 0.15),
    (gen_random_uuid(), 'USDC', 0.10, 0.20, 0.40, 0.20, 0.10),
    (gen_random_uuid(), 'GHS',  0.10, 0.20, 0.40, 0.20, 0.10),
    (gen_random_uuid(), 'ZAR',  0.10, 0.20, 0.40, 0.20, 0.10)
ON CONFLICT (currency) DO NOTHING;
