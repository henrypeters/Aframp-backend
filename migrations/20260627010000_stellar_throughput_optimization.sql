-- Issue #401: Stellar Transaction Throughput Optimization
-- Adds async submission queue, batch tracking, and forensic failure logs.

-- -----------------------------------------------------------------------------
-- Submission queue lifecycle: PENDING -> SUBMITTED -> CONFIRMED / FAILED / RETRYING
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS stellar_submission_queue (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    issuer_id           UUID NOT NULL REFERENCES stellar_issuer_accounts(id) ON DELETE CASCADE,
    channel_id          UUID REFERENCES stellar_submission_channels(id) ON DELETE SET NULL,

    tx_envelope_hash    TEXT NOT NULL,
    tx_envelope_xdr     TEXT NOT NULL,
    operation_count     INTEGER NOT NULL DEFAULT 1 CHECK (operation_count > 0 AND operation_count <= 100),

    queue_status        TEXT NOT NULL CHECK (queue_status IN ('PENDING', 'SUBMITTED', 'CONFIRMED', 'FAILED', 'RETRYING')),
    submission_attempt  INTEGER NOT NULL DEFAULT 0,

    last_error_code     TEXT,
    last_error_reason   TEXT,
    next_attempt_at     TIMESTAMPTZ,

    submitted_at        TIMESTAMPTZ,
    confirmed_at        TIMESTAMPTZ,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (issuer_id, tx_envelope_hash)
);

CREATE INDEX IF NOT EXISTS idx_stellar_submission_queue_status
    ON stellar_submission_queue (issuer_id, queue_status, created_at);

CREATE INDEX IF NOT EXISTS idx_stellar_submission_queue_retry
    ON stellar_submission_queue (next_attempt_at)
    WHERE queue_status IN ('PENDING', 'RETRYING', 'SUBMITTED');

CREATE INDEX IF NOT EXISTS idx_stellar_submission_queue_confirmed
    ON stellar_submission_queue (confirmed_at DESC)
    WHERE queue_status = 'CONFIRMED';

-- -----------------------------------------------------------------------------
-- Forensic failures (canonical reason logging for txBAD_SEQ / txINSUFFICIENT_FEE)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS stellar_tx_forensic_failures (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    queue_id            UUID REFERENCES stellar_submission_queue(id) ON DELETE SET NULL,
    tx_log_id           UUID REFERENCES stellar_transaction_logs(id) ON DELETE SET NULL,
    issuer_id           UUID NOT NULL REFERENCES stellar_issuer_accounts(id) ON DELETE CASCADE,
    channel_id          UUID REFERENCES stellar_submission_channels(id) ON DELETE SET NULL,

    error_code          TEXT NOT NULL,
    error_reason        TEXT NOT NULL,
    horizon_status      TEXT,
    retryable           BOOLEAN NOT NULL DEFAULT FALSE,

    occurred_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_stellar_tx_forensic_failures_code
    ON stellar_tx_forensic_failures (error_code, occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_stellar_tx_forensic_failures_issuer
    ON stellar_tx_forensic_failures (issuer_id, occurred_at DESC);

COMMENT ON TABLE stellar_submission_queue IS
    'Asynchronous throughput queue for Stellar submissions (PENDING->SUBMITTED->CONFIRMED/FAILED/RETRYING).';
COMMENT ON TABLE stellar_tx_forensic_failures IS
    'Canonical forensic log of submission failures (e.g. txBAD_SEQ, txINSUFFICIENT_FEE, txTOO_LATE).';
