-- Create reconciliation tables for transaction reconciliation

DO $$ BEGIN
    CREATE TYPE discrepancy_type AS ENUM (
        'MISSING_MINT',
        'UNAUTHORIZED_MINT',
        'AMOUNT_MISMATCH',
        'DUPLICATE_PAYMENT'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

DO $$ BEGIN
    CREATE TYPE discrepancy_status AS ENUM (
        'OPEN',
        'INVESTIGATING',
        'RESOLVED',
        'ESCALATED'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

CREATE TABLE IF NOT EXISTS discrepancy_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    transaction_id UUID,
    discrepancy_type discrepancy_type NOT NULL,
    fiat_amount DECIMAL(20,8),
    mint_amount DECIMAL(20,8),
    stellar_tx_hash VARCHAR(64),
    payment_reference VARCHAR(255),
    status discrepancy_status NOT NULL DEFAULT 'OPEN',
    detected_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMP WITH TIME ZONE,
    resolved_by UUID,
    resolution_notes TEXT
);

CREATE TABLE IF NOT EXISTS reconciliation_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    report_date DATE NOT NULL UNIQUE,
    total_transactions INTEGER NOT NULL DEFAULT 0,
    matched_count INTEGER NOT NULL DEFAULT 0,
    discrepancy_count INTEGER NOT NULL DEFAULT 0,
    missing_mint_count INTEGER NOT NULL DEFAULT 0,
    unauthorized_mint_count INTEGER NOT NULL DEFAULT 0,
    amount_mismatch_count INTEGER NOT NULL DEFAULT 0,
    duplicate_payment_count INTEGER NOT NULL DEFAULT 0,
    has_open_discrepancies BOOLEAN NOT NULL DEFAULT FALSE,
    period_closed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_discrepancy_log_transaction ON discrepancy_log(transaction_id);
CREATE INDEX IF NOT EXISTS idx_discrepancy_log_status ON discrepancy_log(status);
CREATE INDEX IF NOT EXISTS idx_discrepancy_log_detected ON discrepancy_log(detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_discrepancy_log_type ON discrepancy_log(discrepancy_type);
CREATE INDEX IF NOT EXISTS idx_reconciliation_reports_date ON reconciliation_reports(report_date DESC);

COMMENT ON TABLE discrepancy_log IS 'Logs transaction discrepancies found during reconciliation';
COMMENT ON TABLE reconciliation_reports IS 'Daily reconciliation summary reports';
