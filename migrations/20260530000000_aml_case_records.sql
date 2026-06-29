-- Migration: AML case records storage
-- Stores full AMLCaseRecord payloads as JSONB for flexible persistence
CREATE TABLE IF NOT EXISTS aml_case_records (
    id UUID PRIMARY KEY,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_aml_case_records_updated_at ON aml_case_records (updated_at DESC);

-- Checklist items completion per case
CREATE TABLE IF NOT EXISTS aml_case_checklist_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id UUID NOT NULL REFERENCES aml_case_records(id) ON DELETE CASCADE,
    item_id UUID NOT NULL,
    completed_by TEXT NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_aml_case_checklist_case ON aml_case_checklist_items (case_id);

-- Evidence, notes, and actions for cases
CREATE TABLE IF NOT EXISTS aml_case_evidence (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id UUID NOT NULL REFERENCES aml_case_records(id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    added_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS aml_case_notes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id UUID NOT NULL REFERENCES aml_case_records(id) ON DELETE CASCADE,
    payload JSONB NOT NULL,
    added_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS aml_case_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id UUID NOT NULL REFERENCES aml_case_records(id) ON DELETE CASCADE,
    action_type TEXT NOT NULL,
    action_detail TEXT NOT NULL,
    performed_by TEXT NOT NULL,
    action_timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_aml_case_actions_case ON aml_case_actions (case_id, action_timestamp DESC);

-- updated_at trigger
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN NEW.updated_at = NOW(); RETURN NEW; END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS aml_case_records_updated_at ON aml_case_records;
CREATE TRIGGER aml_case_records_updated_at
    BEFORE UPDATE ON aml_case_records
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
