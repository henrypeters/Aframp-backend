-- Redemption Flow Schema for cNGN Token Burn and Fiat Settlement
-- Fixed order: batches created before requests to avoid FK cycle

-- 1. Redemption Statuses
CREATE TABLE IF NOT EXISTS redemption_statuses (
    code TEXT PRIMARY KEY,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO redemption_statuses (code, description) VALUES
    ('REDEMPTION_REQUESTED', 'User has requested to burn cNGN tokens'),
    ('KYC_VERIFICATION', 'Verifying user KYC status'),
    ('BALANCE_VERIFICATION', 'Verifying on-chain cNGN balance'),
    ('BANK_VALIDATION', 'Validating destination bank account'),
    ('TOKENS_LOCKED', 'cNGN tokens moved to escrow/pending burn'),
    ('BURNING_IN_PROGRESS', 'Burn transaction submitted to Stellar'),
    ('BURNED_CONFIRMED', 'Burn transaction confirmed on-chain'),
    ('FIAT_DISBURSEMENT_PENDING', 'Awaiting fiat transfer'),
    ('FIAT_DISBURSED', 'NGN successfully transferred to user'),
    ('MANUAL_REVIEW', 'Requires manual intervention'),
    ('FAILED', 'Redemption process failed'),
    ('CANCELLED', 'Redemption request cancelled')
ON CONFLICT (code) DO NOTHING;

-- 2. Redemption Batches (must exist before redemption_requests FK)
CREATE TABLE IF NOT EXISTS redemption_batches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    batch_id TEXT NOT NULL UNIQUE,
    total_requests INTEGER NOT NULL DEFAULT 0,
    total_amount_cngn NUMERIC(36,18) NOT NULL DEFAULT 0,
    total_amount_ngn NUMERIC(36,18) NOT NULL DEFAULT 0,
    batch_type TEXT NOT NULL DEFAULT 'MANUAL' CHECK (batch_type IN ('TIME_BASED','COUNT_BASED','MANUAL')),
    trigger_reason TEXT,
    status TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','PROCESSING','COMPLETED','FAILED','PARTIAL')),
    stellar_transaction_hash TEXT,
    stellar_ledger INTEGER,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

-- 3. Redemption Requests
CREATE TABLE IF NOT EXISTS redemption_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    redemption_id TEXT NOT NULL UNIQUE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    wallet_address VARCHAR(255) NOT NULL,
    amount_cngn NUMERIC(36,18) NOT NULL CHECK (amount_cngn > 0),
    amount_ngn NUMERIC(36,18) NOT NULL CHECK (amount_ngn > 0),
    exchange_rate NUMERIC(36,18) NOT NULL CHECK (exchange_rate > 0),
    bank_code TEXT NOT NULL,
    bank_name TEXT NOT NULL,
    account_number TEXT NOT NULL,
    account_name TEXT NOT NULL,
    account_name_verified BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'REDEMPTION_REQUESTED' REFERENCES redemption_statuses(code),
    previous_status TEXT REFERENCES redemption_statuses(code),
    burn_transaction_hash TEXT,
    batch_id UUID REFERENCES redemption_batches(id),
    kyc_tier TEXT CHECK (kyc_tier IN ('TIER_1','TIER_2','TIER_3')),
    ip_address INET,
    user_agent TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ
);

-- 4. Burn Transactions
CREATE TABLE IF NOT EXISTS burn_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    redemption_id UUID NOT NULL REFERENCES redemption_requests(id) ON DELETE CASCADE,
    transaction_hash TEXT NOT NULL UNIQUE,
    stellar_ledger INTEGER,
    sequence_number BIGINT,
    burn_type TEXT NOT NULL DEFAULT 'PAYMENT_TO_ISSUER' CHECK (burn_type IN ('PAYMENT_TO_ISSUER','CLAWBACK')),
    source_address VARCHAR(255) NOT NULL,
    destination_address VARCHAR(255) NOT NULL,
    amount_cngn NUMERIC(36,18) NOT NULL CHECK (amount_cngn > 0),
    status TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','SUCCESS','FAILED','TIMEOUT')),
    fee_paid_stroops INTEGER,
    fee_xlm NUMERIC(36,18),
    timeout_seconds INTEGER NOT NULL DEFAULT 300,
    error_code TEXT,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    unsigned_envelope_xdr TEXT,
    signed_envelope_xdr TEXT,
    memo_text TEXT,
    memo_hash TEXT,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    submitted_at TIMESTAMPTZ,
    confirmed_at TIMESTAMPTZ
);

-- 5. Fiat Disbursements
CREATE TABLE IF NOT EXISTS fiat_disbursements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    redemption_id UUID NOT NULL REFERENCES redemption_requests(id) ON DELETE CASCADE,
    batch_id UUID REFERENCES redemption_batches(id),
    amount_ngn NUMERIC(36,18) NOT NULL CHECK (amount_ngn > 0),
    bank_code TEXT NOT NULL,
    bank_name TEXT NOT NULL,
    account_number TEXT NOT NULL,
    account_name TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_reference TEXT UNIQUE,
    provider_status TEXT,
    status TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN (
        'PENDING','PROCESSING','SUCCESS','FAILED','MANUAL_REVIEW','TIMEOUT','REVERSED'
    )),
    nibss_transaction_id TEXT,
    nibss_status TEXT,
    beneficiary_account_credits BOOLEAN DEFAULT FALSE,
    provider_fee NUMERIC(36,18) DEFAULT 0,
    processing_time_seconds INTEGER,
    error_code TEXT,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    receipt_url TEXT,
    idempotency_key TEXT UNIQUE,
    narration TEXT NOT NULL DEFAULT 'cNGN Redemption',
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    last_status_check TIMESTAMPTZ
);

-- 6. Settlement Accounts
CREATE TABLE IF NOT EXISTS settlement_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_name TEXT NOT NULL UNIQUE,
    account_number TEXT NOT NULL,
    bank_code TEXT NOT NULL,
    bank_name TEXT NOT NULL,
    account_type TEXT NOT NULL DEFAULT 'RESERVE' CHECK (account_type IN ('RESERVE','OPERATIONAL','ESCROW')),
    currency TEXT NOT NULL DEFAULT 'NGN',
    current_balance NUMERIC(36,18) NOT NULL DEFAULT 0,
    available_balance NUMERIC(36,18) NOT NULL DEFAULT 0,
    pending_debits NUMERIC(36,18) NOT NULL DEFAULT 0,
    minimum_balance NUMERIC(36,18) NOT NULL DEFAULT 0,
    is_healthy BOOLEAN NOT NULL DEFAULT TRUE,
    last_balance_check TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 7. Redemption Audit Log
CREATE TABLE IF NOT EXISTS redemption_audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    redemption_id UUID REFERENCES redemption_requests(id) ON DELETE CASCADE,
    batch_id UUID REFERENCES redemption_batches(id),
    burn_transaction_id UUID REFERENCES burn_transactions(id) ON DELETE CASCADE,
    disbursement_id UUID REFERENCES fiat_disbursements(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    previous_status TEXT,
    new_status TEXT,
    event_data JSONB NOT NULL DEFAULT '{}',
    user_id UUID REFERENCES users(id),
    ip_address INET,
    user_agent TEXT,
    worker_id TEXT,
    service_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Triggers
DROP TRIGGER IF EXISTS set_updated_at_redemption_statuses ON redemption_statuses;
CREATE TRIGGER set_updated_at_redemption_statuses BEFORE UPDATE ON redemption_statuses FOR EACH ROW EXECUTE FUNCTION set_updated_at();
DROP TRIGGER IF EXISTS set_updated_at_redemption_requests ON redemption_requests;
CREATE TRIGGER set_updated_at_redemption_requests BEFORE UPDATE ON redemption_requests FOR EACH ROW EXECUTE FUNCTION set_updated_at();
DROP TRIGGER IF EXISTS set_updated_at_redemption_batches ON redemption_batches;
CREATE TRIGGER set_updated_at_redemption_batches BEFORE UPDATE ON redemption_batches FOR EACH ROW EXECUTE FUNCTION set_updated_at();
DROP TRIGGER IF EXISTS set_updated_at_burn_transactions ON burn_transactions;
CREATE TRIGGER set_updated_at_burn_transactions BEFORE UPDATE ON burn_transactions FOR EACH ROW EXECUTE FUNCTION set_updated_at();
DROP TRIGGER IF EXISTS set_updated_at_fiat_disbursements ON fiat_disbursements;
CREATE TRIGGER set_updated_at_fiat_disbursements BEFORE UPDATE ON fiat_disbursements FOR EACH ROW EXECUTE FUNCTION set_updated_at();
DROP TRIGGER IF EXISTS set_updated_at_settlement_accounts ON settlement_accounts;
CREATE TRIGGER set_updated_at_settlement_accounts BEFORE UPDATE ON settlement_accounts FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Indexes
CREATE INDEX IF NOT EXISTS idx_redemption_requests_user_id ON redemption_requests(user_id);
CREATE INDEX IF NOT EXISTS idx_redemption_requests_status ON redemption_requests(status);
CREATE INDEX IF NOT EXISTS idx_redemption_requests_created_at ON redemption_requests(created_at);
CREATE INDEX IF NOT EXISTS idx_redemption_requests_batch ON redemption_requests(batch_id);
CREATE INDEX IF NOT EXISTS idx_redemption_batches_status ON redemption_batches(status);
CREATE INDEX IF NOT EXISTS idx_burn_transactions_redemption_id ON burn_transactions(redemption_id);
CREATE INDEX IF NOT EXISTS idx_burn_transactions_status ON burn_transactions(status);
CREATE INDEX IF NOT EXISTS idx_fiat_disbursements_redemption_id ON fiat_disbursements(redemption_id);
CREATE INDEX IF NOT EXISTS idx_fiat_disbursements_status ON fiat_disbursements(status);
CREATE INDEX IF NOT EXISTS idx_redemption_audit_log_redemption_id ON redemption_audit_log(redemption_id);
CREATE INDEX IF NOT EXISTS idx_redemption_audit_log_created_at ON redemption_audit_log(created_at);
