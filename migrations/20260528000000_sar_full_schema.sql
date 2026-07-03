-- Full SAR (Suspicious Activity Report) schema
-- Replaces the minimal sar_workflow migration with the complete schema.

-- Drop old tables if they exist (idempotent re-run)
DROP TABLE IF EXISTS sar_audit_log CASCADE;
DROP TABLE IF EXISTS sar_narratives CASCADE;
DROP TABLE IF EXISTS sar_transactions CASCADE;
DROP TABLE IF EXISTS sar_subjects CASCADE;
DROP TABLE IF EXISTS sar_reports CASCADE;

-- ── Core SAR record ──────────────────────────────────────────────────────────
CREATE TABLE sar_reports (
    id                          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Classification
    sar_type                    TEXT        NOT NULL
                                    CHECK (sar_type IN ('transaction_based','activity_based','threshold_based')),
    status                      TEXT        NOT NULL DEFAULT 'draft'
                                    CHECK (status IN ('draft','under_review','approved','filed','acknowledged','rejected','returned_for_revision')),
    subject_type                TEXT        NOT NULL
                                    CHECK (subject_type IN ('individual','entity')),
    detection_method            TEXT        NOT NULL
                                    CHECK (detection_method IN ('aml_rule_trigger','compliance_officer_judgment','law_enforcement_request','sanctions_match')),

    -- Subject linkage
    subject_kyc_id              UUID,
    subject_wallet_addresses    TEXT[]      NOT NULL DEFAULT '{}',

    -- Activity details
    suspicious_activity_description TEXT    NOT NULL,
    activity_start_date         DATE        NOT NULL,
    activity_end_date           DATE        NOT NULL,
    total_amount_ngn            NUMERIC(20,2) NOT NULL DEFAULT 0,
    transaction_count           INT         NOT NULL DEFAULT 0,
    linked_transaction_ids      UUID[]      NOT NULL DEFAULT '{}',

    -- AML trigger data (pre-populated from AML engine)
    aml_case_id                 UUID,
    aml_risk_score              NUMERIC(5,4),
    triggered_rules             JSONB       NOT NULL DEFAULT '[]',

    -- Workflow actors
    detecting_officer_id        UUID,
    assigned_investigator_id    UUID,
    reviewing_officer_id        UUID,
    approving_officer_id        UUID,

    -- Investigation checklist (JSON flags)
    investigation_checklist     JSONB       NOT NULL DEFAULT '{
        "subject_identity_verified": false,
        "transaction_records_reviewed": false,
        "aml_rules_documented": false,
        "narrative_complete": false,
        "supporting_docs_attached": false,
        "legal_review_complete": false
    }',

    -- Filing
    filing_deadline             DATE        NOT NULL,
    filing_timestamp            TIMESTAMPTZ,
    filing_method               TEXT,
    regulatory_reference_number TEXT,
    rejection_reason            TEXT,

    -- Acknowledgement
    acknowledged_at             TIMESTAMPTZ,
    acknowledgement_reference   TEXT,

    -- Regulatory authority
    authority                   TEXT        NOT NULL DEFAULT 'NFIU'
                                    CHECK (authority IN ('NFIU','CBN')),

    -- Generated document (stored as JSON string for NFIU, XML for CBN)
    generated_document          TEXT,
    document_generated_at       TIMESTAMPTZ,

    -- Confidentiality: data retention
    retention_expires_at        DATE        NOT NULL DEFAULT (CURRENT_DATE + INTERVAL '5 years'),

    created_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sar_status          ON sar_reports (status, created_at DESC);
CREATE INDEX idx_sar_aml_case        ON sar_reports (aml_case_id) WHERE aml_case_id IS NOT NULL;
CREATE INDEX idx_sar_subject_kyc     ON sar_reports (subject_kyc_id) WHERE subject_kyc_id IS NOT NULL;
CREATE INDEX idx_sar_deadline        ON sar_reports (filing_deadline, status);
CREATE INDEX idx_sar_detection       ON sar_reports (detection_method, created_at DESC);

-- ── SAR subjects (one or more per SAR) ──────────────────────────────────────
CREATE TABLE sar_subjects (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    sar_id              UUID        NOT NULL REFERENCES sar_reports(id) ON DELETE CASCADE,
    full_name           TEXT        NOT NULL,
    date_of_birth       DATE,
    nationality         TEXT,
    identification_docs JSONB       NOT NULL DEFAULT '[]',  -- [{type, number, issuer, expiry}]
    address             TEXT,
    contact_info        JSONB       NOT NULL DEFAULT '{}',  -- {phone, email}
    platform_relationship TEXT      NOT NULL DEFAULT 'account_holder',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sar_subjects_sar ON sar_subjects (sar_id);

-- ── SAR transactions (linked suspicious transactions) ────────────────────────
CREATE TABLE sar_transactions (
    id                          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    sar_id                      UUID        NOT NULL REFERENCES sar_reports(id) ON DELETE CASCADE,
    transaction_id              UUID        NOT NULL,
    transaction_date            TIMESTAMPTZ NOT NULL,
    amount_ngn                  NUMERIC(20,2) NOT NULL,
    transaction_type            TEXT        NOT NULL,
    counterparty_details        JSONB       NOT NULL DEFAULT '{}',
    suspicious_element          TEXT        NOT NULL,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sar_txns_sar ON sar_transactions (sar_id);
CREATE UNIQUE INDEX idx_sar_txns_unique ON sar_transactions (sar_id, transaction_id);

-- ── SAR narratives (versioned) ───────────────────────────────────────────────
CREATE TABLE sar_narratives (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    sar_id          UUID        NOT NULL REFERENCES sar_reports(id) ON DELETE CASCADE,
    version         INT         NOT NULL,
    narrative_text  TEXT        NOT NULL,
    author_id       UUID        NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (sar_id, version)
);

CREATE INDEX idx_sar_narratives_sar ON sar_narratives (sar_id, version DESC);

-- ── SAR audit log (immutable) ────────────────────────────────────────────────
CREATE TABLE sar_audit_log (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    sar_id          UUID        NOT NULL REFERENCES sar_reports(id),
    actor_id        TEXT        NOT NULL,
    action          TEXT        NOT NULL,
    from_status     TEXT        NOT NULL DEFAULT '',
    to_status       TEXT        NOT NULL DEFAULT '',
    notes           TEXT,
    -- Confidentiality: record who accessed the SAR
    access_type     TEXT        NOT NULL DEFAULT 'write',  -- 'read' | 'write'
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sar_audit_sar_id ON sar_audit_log (sar_id, created_at ASC);

-- Immutability trigger
CREATE OR REPLACE FUNCTION sar_audit_log_immutable()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'sar_audit_log is immutable';
END;
$$;

CREATE TRIGGER trg_sar_audit_immutable
    BEFORE UPDATE OR DELETE ON sar_audit_log
    FOR EACH ROW EXECUTE FUNCTION sar_audit_log_immutable();

-- updated_at trigger
CREATE OR REPLACE FUNCTION update_sar_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN NEW.updated_at = NOW(); RETURN NEW; END;
$$;

CREATE TRIGGER sar_reports_updated_at
    BEFORE UPDATE ON sar_reports
    FOR EACH ROW EXECUTE FUNCTION update_sar_updated_at();

COMMENT ON TABLE sar_reports     IS 'Full SAR lifecycle: draft→under_review→approved→filed→acknowledged';
COMMENT ON TABLE sar_subjects    IS 'Subjects named in a SAR (individual or entity)';
COMMENT ON TABLE sar_transactions IS 'Suspicious transactions linked to a SAR';
COMMENT ON TABLE sar_narratives  IS 'Versioned narrative text for each SAR';
COMMENT ON TABLE sar_audit_log   IS 'Immutable audit trail — every SAR access and state change';
