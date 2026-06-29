-- AML Programme Effectiveness Reporting & Metrics (Issue #396)
-- Adds KPI snapshotting, benchmarking, QC sampling, and richer quarterly report payloads.

-- -----------------------------------------------------------------------------
-- 1) Hourly KPI snapshots (dashboard refresh >= hourly)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS aml_effectiveness_metric_snapshots (
    snapshot_at                          TIMESTAMPTZ NOT NULL,
    corridor_id                          TEXT NOT NULL DEFAULT 'all',
    user_tier                            TEXT NOT NULL DEFAULT 'all',
    rule_set                             TEXT NOT NULL DEFAULT 'all',
    asset_class                          TEXT NOT NULL DEFAULT 'all',

    total_alerts                         BIGINT NOT NULL DEFAULT 0,
    sar_conversion_rate                  DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    alert_processing_time_hours          DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    false_positive_ratio                 DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    high_risk_jurisdiction_coverage      DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    policy_override_frequency            DOUBLE PRECISION NOT NULL DEFAULT 0.0,

    refreshed_at                         TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (snapshot_at, corridor_id, user_tier, rule_set, asset_class)
);

CREATE INDEX IF NOT EXISTS idx_aml_eff_snapshots_time
    ON aml_effectiveness_metric_snapshots (snapshot_at DESC);

-- -----------------------------------------------------------------------------
-- 2) Benchmark catalogue (industry or internal baselines)
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS aml_metric_benchmarks (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    metric_name         TEXT NOT NULL,
    benchmark_scope     TEXT NOT NULL CHECK (benchmark_scope IN ('industry', 'internal_baseline')),
    benchmark_value     DOUBLE PRECISION NOT NULL,
    source              TEXT,
    period_months       INTEGER NOT NULL DEFAULT 6,
    effective_from      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    effective_to        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_aml_metric_benchmarks_lookup
    ON aml_metric_benchmarks (metric_name, benchmark_scope, effective_from DESC);

-- Optional default industry placeholders; values can be tuned by compliance later.
INSERT INTO aml_metric_benchmarks (metric_name, benchmark_scope, benchmark_value, source)
VALUES
    ('sar_conversion_rate', 'industry', 0.08, 'industry_placeholder'),
    ('alert_processing_time_hours', 'industry', 36.0, 'industry_placeholder'),
    ('false_positive_ratio', 'industry', 0.70, 'industry_placeholder'),
    ('high_risk_jurisdiction_coverage', 'industry', 0.60, 'industry_placeholder'),
    ('policy_override_frequency', 'industry', 0.03, 'industry_placeholder')
ON CONFLICT DO NOTHING;

-- -----------------------------------------------------------------------------
-- 3) QC sampling assignments for dismissed alerts
-- -----------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS aml_alert_qc_reviews (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    aml_case_id             UUID NOT NULL REFERENCES aml_cases(id) ON DELETE CASCADE,
    sampled_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    assigned_reviewer_id    TEXT NOT NULL,
    review_status           TEXT NOT NULL DEFAULT 'pending'
                                CHECK (review_status IN ('pending', 'completed', 'escalated')),
    review_outcome          TEXT
                                CHECK (review_outcome IN ('confirmed_false_positive', 'missed_suspicious_activity', 'inconclusive')),
    notes                   TEXT,
    completed_at            TIMESTAMPTZ,

    UNIQUE (aml_case_id)
);

CREATE INDEX IF NOT EXISTS idx_aml_alert_qc_reviews_sampled
    ON aml_alert_qc_reviews (sampled_at DESC);

CREATE INDEX IF NOT EXISTS idx_aml_alert_qc_reviews_status
    ON aml_alert_qc_reviews (review_status, sampled_at DESC);

-- -----------------------------------------------------------------------------
-- 4) Enrich quarterly report payload fields in existing report table
-- -----------------------------------------------------------------------------
ALTER TABLE compliance_effectiveness_reports
    ADD COLUMN IF NOT EXISTS kpi_payload JSONB NOT NULL DEFAULT '{}'::JSONB,
    ADD COLUMN IF NOT EXISTS policy_effectiveness_payload JSONB NOT NULL DEFAULT '[]'::JSONB,
    ADD COLUMN IF NOT EXISTS heatmap_payload JSONB NOT NULL DEFAULT '[]'::JSONB,
    ADD COLUMN IF NOT EXISTS benchmark_payload JSONB NOT NULL DEFAULT '[]'::JSONB,
    ADD COLUMN IF NOT EXISTS trend_alerts_payload JSONB NOT NULL DEFAULT '[]'::JSONB,
    ADD COLUMN IF NOT EXISTS policy_adjustments TEXT[] NOT NULL DEFAULT '{}';

COMMENT ON TABLE aml_effectiveness_metric_snapshots IS 'Hourly AML programme effectiveness KPI snapshots for dashboard and trend analysis';
COMMENT ON TABLE aml_metric_benchmarks IS 'Benchmark values for AML KPI comparison (industry/internal baseline)';
COMMENT ON TABLE aml_alert_qc_reviews IS 'QC review assignments sampled from dismissed AML alerts (5-10%)';
