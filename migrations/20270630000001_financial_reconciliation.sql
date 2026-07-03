-- ============================================================================
-- Financial Reconciliation Infrastructure
-- ============================================================================
-- Implements hourly ledger drift detection, on-chain vs off-chain balance
-- verification, and automated circuit-breaker safety controls.
-- ============================================================================

-- ============================================================================
-- 1. Reconciliation Ledger Snapshots
-- ============================================================================

CREATE TABLE IF NOT EXISTS reconciliation_ledger_snaps (
    id BIGSERIAL PRIMARY KEY,
    snapshot_time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Internal ledger state
    internal_balance_stroops BIGINT NOT NULL, -- 7 decimal places
    internal_transaction_count BIGINT NOT NULL,
    internal_last_tx_id UUID,
    
    -- On-chain Stellar state
    stellar_balance_stroops BIGINT NOT NULL,
    stellar_sequence_number BIGINT,
    stellar_account_id VARCHAR(56) NOT NULL,
    
    -- Reconciliation results
    balance_drift_stroops BIGINT NOT NULL, -- Difference: internal - stellar
    drift_percentage DECIMAL(10, 7),
    is_reconciled BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Metadata
    reconciliation_duration_ms INTEGER,
    transactions_verified INTEGER,
    error_message TEXT,
    metadata JSONB,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_recon_snap_time ON reconciliation_ledger_snaps(snapshot_time DESC);
CREATE INDEX idx_recon_snap_account ON reconciliation_ledger_snaps(stellar_account_id, snapshot_time DESC);
CREATE INDEX idx_recon_snap_drift ON reconciliation_ledger_snaps(is_reconciled, snapshot_time DESC) 
    WHERE NOT is_reconciled;
CREATE INDEX idx_recon_snap_large_drift ON reconciliation_ledger_snaps(ABS(balance_drift_stroops) DESC, snapshot_time DESC)
    WHERE ABS(balance_drift_stroops) > 100000; -- > 0.01 XLM

-- ============================================================================
-- 2. Transaction-Level Reconciliation
-- ============================================================================

CREATE TABLE IF NOT EXISTS reconciliation_transaction_audit (
    id BIGSERIAL PRIMARY KEY,
    snapshot_id BIGINT NOT NULL REFERENCES reconciliation_ledger_snaps(id),
    
    -- Transaction identification
    internal_tx_id UUID NOT NULL,
    stellar_tx_hash VARCHAR(64),
    
    -- Transaction details
    amount_stroops BIGINT NOT NULL,
    operation_type VARCHAR(50) NOT NULL, -- 'MINT', 'BURN', 'TRANSFER'
    source_account VARCHAR(56),
    destination_account VARCHAR(56),
    
    -- Reconciliation status
    found_on_chain BOOLEAN NOT NULL DEFAULT FALSE,
    found_in_ledger BOOLEAN NOT NULL DEFAULT TRUE,
    amounts_match BOOLEAN,
    
    -- Discrepancy details
    discrepancy_type VARCHAR(100), -- 'MISSING_ON_CHAIN', 'MISSING_IN_LEDGER', 'AMOUNT_MISMATCH'
    discrepancy_notes TEXT,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_recon_tx_snapshot ON reconciliation_transaction_audit(snapshot_id);
CREATE INDEX idx_recon_tx_internal ON reconciliation_transaction_audit(internal_tx_id);
CREATE INDEX idx_recon_tx_stellar ON reconciliation_transaction_audit(stellar_tx_hash);
CREATE INDEX idx_recon_tx_discrepancy ON reconciliation_transaction_audit(discrepancy_type, created_at DESC)
    WHERE discrepancy_type IS NOT NULL;

-- ============================================================================
-- 3. Circuit Breaker Status
-- ============================================================================

CREATE TABLE IF NOT EXISTS reconciliation_circuit_breaker (
    id BIGSERIAL PRIMARY KEY,
    
    -- Circuit breaker identification
    circuit_name VARCHAR(100) NOT NULL UNIQUE, -- e.g., 'NGN_CORRIDOR', 'KES_CORRIDOR'
    
    -- Status
    is_tripped BOOLEAN NOT NULL DEFAULT FALSE,
    trip_reason VARCHAR(255),
    tripped_at TIMESTAMPTZ,
    
    -- Thresholds
    max_drift_stroops BIGINT NOT NULL DEFAULT 50000000, -- 5 XLM = 5 * 10^7 stroops
    max_drift_percentage DECIMAL(5, 2) NOT NULL DEFAULT 0.50, -- 0.5%
    
    -- Auto-recovery
    auto_recovery_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    recovery_threshold_stroops BIGINT,
    last_recovery_check TIMESTAMPTZ,
    
    -- Operational impact
    operations_blocked_count BIGINT NOT NULL DEFAULT 0,
    last_block_attempt TIMESTAMPTZ,
    
    -- Metadata
    metadata JSONB,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_circuit_breaker_status ON reconciliation_circuit_breaker(is_tripped, updated_at DESC);
CREATE INDEX idx_circuit_breaker_name ON reconciliation_circuit_breaker(circuit_name);

-- Insert default circuit breakers
INSERT INTO reconciliation_circuit_breaker (
    circuit_name, max_drift_stroops, max_drift_percentage
) VALUES 
    ('GLOBAL_RECONCILIATION', 100000000, 1.0), -- 10 XLM, 1%
    ('NGN_CORRIDOR', 50000000, 0.5),           -- 5 XLM, 0.5%
    ('KES_CORRIDOR', 50000000, 0.5),
    ('GHS_CORRIDOR', 50000000, 0.5),
    ('UGX_CORRIDOR', 50000000, 0.5)
ON CONFLICT (circuit_name) DO NOTHING;

-- ============================================================================
-- 4. Circuit Breaker Event Log
-- ============================================================================

CREATE TABLE IF NOT EXISTS circuit_breaker_events (
    id BIGSERIAL PRIMARY KEY,
    circuit_breaker_id BIGINT NOT NULL REFERENCES reconciliation_circuit_breaker(id),
    
    event_type VARCHAR(50) NOT NULL, -- 'TRIP', 'RESET', 'BLOCK_ATTEMPT'
    
    -- Event details
    triggered_by VARCHAR(100), -- 'RECONCILIATION_WORKER', 'MANUAL_OPERATOR', 'AUTO_RECOVERY'
    drift_stroops BIGINT,
    drift_percentage DECIMAL(10, 7),
    
    -- Context
    snapshot_id BIGINT REFERENCES reconciliation_ledger_snaps(id),
    operator_user_id UUID,
    reason TEXT,
    
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_cb_events_breaker ON circuit_breaker_events(circuit_breaker_id, created_at DESC);
CREATE INDEX idx_cb_events_type ON circuit_breaker_events(event_type, created_at DESC);

-- ============================================================================
-- 5. Reconciliation Functions
-- ============================================================================

-- Function to check if circuit breaker should trip
CREATE OR REPLACE FUNCTION check_circuit_breaker_thresholds(
    p_circuit_name VARCHAR,
    p_drift_stroops BIGINT,
    p_total_balance_stroops BIGINT
) RETURNS BOOLEAN AS $$
DECLARE
    v_breaker RECORD;
    v_drift_percentage DECIMAL(10, 7);
    v_should_trip BOOLEAN := FALSE;
BEGIN
    -- Get circuit breaker configuration
    SELECT * INTO v_breaker
    FROM reconciliation_circuit_breaker
    WHERE circuit_name = p_circuit_name;
    
    IF NOT FOUND THEN
        RAISE WARNING 'Circuit breaker not found: %', p_circuit_name;
        RETURN FALSE;
    END IF;
    
    -- Calculate drift percentage
    IF p_total_balance_stroops > 0 THEN
        v_drift_percentage := (ABS(p_drift_stroops)::DECIMAL / p_total_balance_stroops) * 100;
    ELSE
        v_drift_percentage := 0;
    END IF;
    
    -- Check thresholds
    IF ABS(p_drift_stroops) > v_breaker.max_drift_stroops THEN
        v_should_trip := TRUE;
    END IF;
    
    IF v_drift_percentage > v_breaker.max_drift_percentage THEN
        v_should_trip := TRUE;
    END IF;
    
    RETURN v_should_trip;
END;
$$ LANGUAGE plpgsql;

-- Function to trip circuit breaker
CREATE OR REPLACE FUNCTION trip_circuit_breaker(
    p_circuit_name VARCHAR,
    p_reason TEXT,
    p_drift_stroops BIGINT,
    p_snapshot_id BIGINT
) RETURNS VOID AS $$
DECLARE
    v_breaker_id BIGINT;
    v_drift_percentage DECIMAL(10, 7);
BEGIN
    -- Update circuit breaker status
    UPDATE reconciliation_circuit_breaker
    SET 
        is_tripped = TRUE,
        trip_reason = p_reason,
        tripped_at = NOW(),
        updated_at = NOW()
    WHERE circuit_name = p_circuit_name
    RETURNING id INTO v_breaker_id;
    
    -- Calculate drift percentage from snapshot
    SELECT 
        drift_percentage INTO v_drift_percentage
    FROM reconciliation_ledger_snaps
    WHERE id = p_snapshot_id;
    
    -- Log event
    INSERT INTO circuit_breaker_events (
        circuit_breaker_id, event_type, triggered_by,
        drift_stroops, drift_percentage,
        snapshot_id, reason
    ) VALUES (
        v_breaker_id, 'TRIP', 'RECONCILIATION_WORKER',
        p_drift_stroops, v_drift_percentage,
        p_snapshot_id, p_reason
    );
    
    RAISE WARNING 'Circuit breaker tripped: % - %', p_circuit_name, p_reason;
END;
$$ LANGUAGE plpgsql;

-- Function to reset circuit breaker (manual operator action)
CREATE OR REPLACE FUNCTION reset_circuit_breaker(
    p_circuit_name VARCHAR,
    p_operator_user_id UUID,
    p_reason TEXT
) RETURNS VOID AS $$
DECLARE
    v_breaker_id BIGINT;
BEGIN
    -- Update circuit breaker status
    UPDATE reconciliation_circuit_breaker
    SET 
        is_tripped = FALSE,
        trip_reason = NULL,
        tripped_at = NULL,
        updated_at = NOW()
    WHERE circuit_name = p_circuit_name
    RETURNING id INTO v_breaker_id;
    
    -- Log event
    INSERT INTO circuit_breaker_events (
        circuit_breaker_id, event_type, triggered_by,
        operator_user_id, reason
    ) VALUES (
        v_breaker_id, 'RESET', 'MANUAL_OPERATOR',
        p_operator_user_id, p_reason
    );
    
    RAISE NOTICE 'Circuit breaker reset: %', p_circuit_name;
END;
$$ LANGUAGE plpgsql;

-- Function to check if operations are allowed
CREATE OR REPLACE FUNCTION is_circuit_breaker_tripped(
    p_circuit_name VARCHAR
) RETURNS BOOLEAN AS $$
DECLARE
    v_is_tripped BOOLEAN;
BEGIN
    SELECT is_tripped INTO v_is_tripped
    FROM reconciliation_circuit_breaker
    WHERE circuit_name = p_circuit_name;
    
    IF NOT FOUND THEN
        RETURN FALSE; -- Allow if breaker doesn't exist
    END IF;
    
    -- Log block attempt if tripped
    IF v_is_tripped THEN
        UPDATE reconciliation_circuit_breaker
        SET 
            operations_blocked_count = operations_blocked_count + 1,
            last_block_attempt = NOW()
        WHERE circuit_name = p_circuit_name;
        
        INSERT INTO circuit_breaker_events (
            circuit_breaker_id, event_type, triggered_by
        ) SELECT id, 'BLOCK_ATTEMPT', 'SYSTEM'
        FROM reconciliation_circuit_breaker
        WHERE circuit_name = p_circuit_name;
    END IF;
    
    RETURN COALESCE(v_is_tripped, FALSE);
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- 6. Monitoring Views
-- ============================================================================

CREATE OR REPLACE VIEW v_reconciliation_dashboard AS
SELECT 
    DATE_TRUNC('hour', snapshot_time) AS hour,
    COUNT(*) AS total_snapshots,
    SUM(CASE WHEN is_reconciled THEN 1 ELSE 0 END) AS reconciled_count,
    SUM(CASE WHEN NOT is_reconciled THEN 1 ELSE 0 END) AS unreconciled_count,
    AVG(ABS(balance_drift_stroops))::BIGINT AS avg_drift_stroops,
    MAX(ABS(balance_drift_stroops)) AS max_drift_stroops,
    AVG(reconciliation_duration_ms)::INTEGER AS avg_duration_ms,
    SUM(transactions_verified) AS total_transactions_verified
FROM reconciliation_ledger_snaps
WHERE snapshot_time > NOW() - INTERVAL '24 hours'
GROUP BY DATE_TRUNC('hour', snapshot_time)
ORDER BY hour DESC;

CREATE OR REPLACE VIEW v_circuit_breaker_status AS
SELECT 
    cb.circuit_name,
    cb.is_tripped,
    cb.trip_reason,
    cb.tripped_at,
    cb.operations_blocked_count,
    cb.max_drift_stroops,
    cb.max_drift_percentage,
    (
        SELECT COUNT(*) 
        FROM circuit_breaker_events cbe 
        WHERE cbe.circuit_breaker_id = cb.id 
        AND cbe.event_type = 'TRIP'
        AND cbe.created_at > NOW() - INTERVAL '24 hours'
    ) AS trips_last_24h
FROM reconciliation_circuit_breaker cb
ORDER BY cb.is_tripped DESC, cb.circuit_name;

COMMENT ON TABLE reconciliation_ledger_snaps IS 'Hourly snapshots comparing internal ledger with on-chain Stellar balances';
COMMENT ON TABLE reconciliation_circuit_breaker IS 'Circuit breaker configuration and status for financial corridors';
COMMENT ON TABLE circuit_breaker_events IS 'Audit log of all circuit breaker state changes';
