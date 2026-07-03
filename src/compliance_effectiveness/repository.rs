use crate::compliance_effectiveness::models::{
    AlertQualityControlStats, BenchmarkComparison, MetricFilters, PolicyEffectivenessMetric,
    QuarterlyEffectivenessReport, RealtimeComplianceKpis, RiskHeatmapCell, SarTrendPoint,
};
use anyhow::Context;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct ComplianceEffectivenessRepository {
    pool: PgPool,
}

impl ComplianceEffectivenessRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn compute_realtime_kpis(
        &self,
        filters: &MetricFilters,
    ) -> Result<RealtimeComplianceKpis, anyhow::Error> {
        let row = sqlx::query(
            r#"
            WITH filtered_cases AS (
                SELECT
                    a.id,
                    a.status,
                    a.flag_level,
                    a.created_at,
                    a.updated_at,
                    a.flags_json,
                    t.metadata AS tx_metadata
                FROM aml_cases a
                LEFT JOIN transactions t ON t.transaction_id = a.transaction_id
                WHERE a.created_at >= $1
                  AND a.created_at < $2
                  AND ($3::text IS NULL OR t.metadata->>'corridor_id' = $3)
                  AND ($4::text IS NULL OR t.metadata->>'user_tier' = $4)
                  AND ($5::text IS NULL OR t.metadata->>'asset_class' = $5)
                  AND (
                        $6::text IS NULL
                        OR a.flags_json::text ILIKE '%' || $6 || '%'
                  )
            ),
            sar_link AS (
                SELECT DISTINCT aml_case_id
                FROM sar_reports
                WHERE aml_case_id IS NOT NULL
                  AND (filing_timestamp IS NOT NULL OR status IN ('filed', 'acknowledged'))
            )
            SELECT
                COUNT(*)::bigint AS total_alerts,
                COUNT(*) FILTER (WHERE fc.status = 'Cleared')::bigint AS false_positives,
                COUNT(*) FILTER (WHERE fc.status IN ('Cleared','PermanentlyBlocked'))::bigint AS resolved_alerts,
                COALESCE(
                    AVG(
                        CASE WHEN fc.status IN ('Cleared','PermanentlyBlocked')
                            THEN EXTRACT(EPOCH FROM (fc.updated_at - fc.created_at))/3600.0
                            ELSE NULL
                        END
                    ),
                    0
                )::double precision AS avg_processing_hrs,
                COUNT(*) FILTER (WHERE sl.aml_case_id IS NOT NULL)::bigint AS sar_conversions,
                COUNT(*) FILTER (WHERE fc.flags_json::text ILIKE '%HighCorridorRisk%')::bigint AS high_risk_alerts
            FROM filtered_cases fc
            LEFT JOIN sar_link sl ON sl.aml_case_id = fc.id
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .bind(filters.corridor_id.as_deref())
        .bind(filters.user_tier.as_deref())
        .bind(filters.asset_class.as_deref())
        .bind(filters.rule_set.as_deref())
        .fetch_one(&self.pool)
        .await?;

        let total_alerts: i64 = row.try_get("total_alerts")?;
        let false_positives: i64 = row.try_get("false_positives")?;
        let avg_processing_hrs: f64 = row.try_get("avg_processing_hrs")?;
        let sar_conversions: i64 = row.try_get("sar_conversions")?;
        let high_risk_alerts: i64 = row.try_get("high_risk_alerts")?;

        let high_risk_total: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint
            FROM transactions t
            WHERE t.created_at >= $1
              AND t.created_at < $2
              AND ($3::text IS NULL OR t.metadata->>'corridor_id' = $3)
              AND ($4::text IS NULL OR t.metadata->>'user_tier' = $4)
              AND ($5::text IS NULL OR t.metadata->>'asset_class' = $5)
              AND EXISTS (
                  SELECT 1
                  FROM aml_corridor_risk_weights rw
                  WHERE rw.weight >= 0.75
                    AND (rw.origin_country || '-' || rw.destination_country) = COALESCE(t.metadata->>'corridor_id', '')
              )
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .bind(filters.corridor_id.as_deref())
        .bind(filters.user_tier.as_deref())
        .bind(filters.asset_class.as_deref())
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let policy_overrides: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint
            FROM aml_case_actions a
            WHERE a.action_timestamp >= $1
              AND a.action_timestamp < $2
              AND lower(a.action_type) IN ('policy_override', 'override')
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let total_alerts_f = total_alerts as f64;
        let sar_conversion_rate = if total_alerts > 0 {
            sar_conversions as f64 / total_alerts_f
        } else {
            0.0
        };
        let false_positive_ratio = if total_alerts > 0 {
            false_positives as f64 / total_alerts_f
        } else {
            0.0
        };
        let high_risk_jurisdiction_coverage = if high_risk_total > 0 {
            high_risk_alerts as f64 / (high_risk_total as f64)
        } else {
            0.0
        };
        let policy_override_frequency = if total_alerts > 0 {
            policy_overrides as f64 / total_alerts_f
        } else {
            0.0
        };

        Ok(RealtimeComplianceKpis {
            total_alerts,
            sar_conversion_rate,
            alert_processing_time_hours: avg_processing_hrs,
            false_positive_ratio,
            high_risk_jurisdiction_coverage,
            policy_override_frequency,
            refreshed_at: Utc::now(),
        })
    }

    pub async fn policy_effectiveness(
        &self,
        filters: &MetricFilters,
        limit: i64,
    ) -> Result<Vec<PolicyEffectivenessMetric>, anyhow::Error> {
        let rows = sqlx::query(
            r#"
            WITH expanded AS (
                SELECT
                    a.id AS case_id,
                    a.status,
                    CASE
                        WHEN jsonb_typeof(rule_obj) = 'object' THEN COALESCE((SELECT key FROM jsonb_each(rule_obj) LIMIT 1), 'unknown_rule')
                        ELSE 'unknown_rule'
                    END AS rule_name
                FROM aml_cases a
                LEFT JOIN transactions t ON t.transaction_id = a.transaction_id
                CROSS JOIN LATERAL jsonb_array_elements(COALESCE(a.flags_json, '[]'::jsonb)) rule_obj
                WHERE a.created_at >= $1
                  AND a.created_at < $2
                  AND ($3::text IS NULL OR t.metadata->>'corridor_id' = $3)
                  AND ($4::text IS NULL OR t.metadata->>'user_tier' = $4)
                  AND ($5::text IS NULL OR t.metadata->>'asset_class' = $5)
                  AND ($6::text IS NULL OR a.flags_json::text ILIKE '%' || $6 || '%')
            ),
            sar_link AS (
                SELECT DISTINCT aml_case_id
                FROM sar_reports
                WHERE aml_case_id IS NOT NULL
                  AND (filing_timestamp IS NOT NULL OR status IN ('filed', 'acknowledged'))
            )
            SELECT
                e.rule_name,
                COUNT(*)::bigint AS total_alerts,
                COUNT(*) FILTER (WHERE sl.aml_case_id IS NOT NULL)::bigint AS sar_conversions,
                COUNT(*) FILTER (WHERE e.status = 'Cleared')::bigint AS false_positives
            FROM expanded e
            LEFT JOIN sar_link sl ON sl.aml_case_id = e.case_id
            GROUP BY e.rule_name
            ORDER BY total_alerts DESC
            LIMIT $7
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .bind(filters.corridor_id.as_deref())
        .bind(filters.user_tier.as_deref())
        .bind(filters.asset_class.as_deref())
        .bind(filters.rule_set.as_deref())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let total_alerts: i64 = row.try_get("total_alerts")?;
            let false_positives: i64 = row.try_get("false_positives")?;
            let sar_conversions: i64 = row.try_get("sar_conversions")?;

            let false_positive_ratio = if total_alerts > 0 {
                false_positives as f64 / total_alerts as f64
            } else {
                0.0
            };
            let noise_index = if total_alerts > 0 {
                false_positive_ratio * (total_alerts as f64).ln_1p()
            } else {
                0.0
            };

            out.push(PolicyEffectivenessMetric {
                rule_name: row.try_get("rule_name")?,
                total_alerts,
                sar_conversions,
                false_positive_ratio,
                noise_index,
            });
        }

        Ok(out)
    }

    pub async fn risk_heatmap(
        &self,
        filters: &MetricFilters,
    ) -> Result<Vec<RiskHeatmapCell>, anyhow::Error> {
        let rows = sqlx::query(
            r#"
            SELECT
                COALESCE(t.metadata->>'corridor_id', 'unknown') AS corridor_id,
                COALESCE(t.metadata->>'user_tier', 'unknown') AS user_tier,
                COALESCE(t.metadata->>'asset_class', t.to_currency, 'unknown') AS asset_class,
                COALESCE(SUM(t.from_amount)::double precision, 0) AS transaction_volume,
                COUNT(a.id) FILTER (WHERE a.flag_level IN ('MEDIUM','CRITICAL'))::bigint AS high_risk_alerts
            FROM transactions t
            LEFT JOIN aml_cases a ON a.transaction_id = t.transaction_id
            WHERE t.created_at >= $1
              AND t.created_at < $2
              AND ($3::text IS NULL OR t.metadata->>'corridor_id' = $3)
              AND ($4::text IS NULL OR t.metadata->>'user_tier' = $4)
              AND ($5::text IS NULL OR COALESCE(t.metadata->>'asset_class', t.to_currency) = $5)
            GROUP BY 1,2,3
            ORDER BY high_risk_alerts DESC, transaction_volume DESC
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .bind(filters.corridor_id.as_deref())
        .bind(filters.user_tier.as_deref())
        .bind(filters.asset_class.as_deref())
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let tx_volume: f64 = row.try_get("transaction_volume")?;
            let high_risk_alerts: i64 = row.try_get("high_risk_alerts")?;
            let risk_intensity = if tx_volume > 0.0 {
                (high_risk_alerts as f64) / tx_volume.max(1.0).sqrt()
            } else {
                high_risk_alerts as f64
            };

            out.push(RiskHeatmapCell {
                corridor_id: row.try_get("corridor_id")?,
                user_tier: row.try_get("user_tier")?,
                asset_class: row.try_get("asset_class")?,
                transaction_volume: tx_volume,
                high_risk_alerts,
                risk_intensity,
            });
        }

        Ok(out)
    }

    pub async fn sar_conversion_trend(
        &self,
        lookback_months: i32,
    ) -> Result<Vec<SarTrendPoint>, anyhow::Error> {
        let rows = sqlx::query(
            r#"
            WITH monthly AS (
                SELECT
                    date_trunc('month', a.created_at)::date AS month,
                    COUNT(*)::bigint AS total_alerts,
                    COUNT(*) FILTER (
                        WHERE EXISTS (
                            SELECT 1
                            FROM sar_reports s
                            WHERE s.aml_case_id = a.id
                              AND (s.filing_timestamp IS NOT NULL OR s.status IN ('filed', 'acknowledged'))
                        )
                    )::bigint AS sar_conversions
                FROM aml_cases a
                WHERE a.created_at >= (date_trunc('month', NOW()) - ($1::int || ' months')::interval)
                GROUP BY 1
                ORDER BY 1 ASC
            )
            SELECT month, total_alerts, sar_conversions FROM monthly
            "#,
        )
        .bind(lookback_months)
        .fetch_all(&self.pool)
        .await?;

        let mut base: Vec<(NaiveDate, f64)> = Vec::with_capacity(rows.len());
        for row in rows {
            let month: NaiveDate = row.try_get("month")?;
            let total: i64 = row.try_get("total_alerts")?;
            let sar: i64 = row.try_get("sar_conversions")?;
            let rate = if total > 0 {
                sar as f64 / total as f64
            } else {
                0.0
            };
            base.push((month, rate));
        }

        let mut out = Vec::with_capacity(base.len());
        for idx in 0..base.len() {
            let (month, conversion_rate) = base[idx];
            let moving_avg_6m = if idx >= 6 {
                let slice = &base[(idx - 6)..idx];
                let avg = slice.iter().map(|(_, r)| *r).sum::<f64>() / 6.0;
                Some(avg)
            } else {
                None
            };
            let deviation_ratio = moving_avg_6m.map(|ma| {
                if ma.abs() < f64::EPSILON {
                    0.0
                } else {
                    (conversion_rate - ma) / ma
                }
            });
            let deviation_alert = deviation_ratio.map(|d| d.abs() >= 0.25).unwrap_or(false);

            out.push(SarTrendPoint {
                month,
                conversion_rate,
                moving_average_6m,
                deviation_ratio,
                deviation_alert,
            });
        }

        Ok(out)
    }

    pub async fn benchmark_comparisons(
        &self,
        kpis: &RealtimeComplianceKpis,
    ) -> Result<Vec<BenchmarkComparison>, anyhow::Error> {
        let rows = sqlx::query(
            r#"
            SELECT metric_name, benchmark_value
            FROM aml_metric_benchmarks
            WHERE benchmark_scope = 'industry'
              AND effective_from <= NOW()
              AND (effective_to IS NULL OR effective_to >= NOW())
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let mut industry_map = std::collections::HashMap::<String, f64>::new();
        for row in rows {
            let name: String = row.try_get("metric_name")?;
            let value: f64 = row.try_get("benchmark_value")?;
            industry_map.insert(name, value);
        }

        let baseline = self.internal_baseline_6m().await?;

        let points = vec![
            ("sar_conversion_rate", kpis.sar_conversion_rate),
            (
                "alert_processing_time_hours",
                kpis.alert_processing_time_hours,
            ),
            ("false_positive_ratio", kpis.false_positive_ratio),
            (
                "high_risk_jurisdiction_coverage",
                kpis.high_risk_jurisdiction_coverage,
            ),
            ("policy_override_frequency", kpis.policy_override_frequency),
        ];

        let mut out = Vec::with_capacity(points.len());
        for (metric, current) in points {
            let internal = *baseline.get(metric).unwrap_or(&current);
            let industry = industry_map.get(metric).copied();

            let status = if metric == "alert_processing_time_hours"
                || metric == "false_positive_ratio"
                || metric == "policy_override_frequency"
            {
                if current <= internal {
                    "good"
                } else {
                    "watch"
                }
            } else if current >= internal {
                "good"
            } else {
                "watch"
            }
            .to_string();

            out.push(BenchmarkComparison {
                metric_name: metric.to_string(),
                current_value: current,
                internal_baseline: internal,
                industry_benchmark: industry,
                status,
            });
        }

        Ok(out)
    }

    async fn internal_baseline_6m(
        &self,
    ) -> Result<std::collections::HashMap<String, f64>, anyhow::Error> {
        let row = sqlx::query(
            r#"
            WITH base AS (
                SELECT
                    COUNT(*)::double precision AS total_alerts,
                    COUNT(*) FILTER (WHERE status = 'Cleared')::double precision AS false_positives,
                    AVG(
                        CASE WHEN status IN ('Cleared','PermanentlyBlocked')
                            THEN EXTRACT(EPOCH FROM (updated_at - created_at))/3600.0
                            ELSE NULL
                        END
                    ) AS processing_hrs,
                    COUNT(*) FILTER (
                        WHERE EXISTS (
                            SELECT 1 FROM sar_reports s
                            WHERE s.aml_case_id = aml_cases.id
                              AND (s.filing_timestamp IS NOT NULL OR s.status IN ('filed','acknowledged'))
                        )
                    )::double precision AS sar_conversions,
                    COUNT(*) FILTER (WHERE flags_json::text ILIKE '%HighCorridorRisk%')::double precision AS high_risk_alerts
                FROM aml_cases
                WHERE created_at >= NOW() - INTERVAL '6 months'
            )
            SELECT
                COALESCE(sar_conversions / NULLIF(total_alerts, 0), 0) AS sar_conversion_rate,
                COALESCE(processing_hrs, 0) AS alert_processing_time_hours,
                COALESCE(false_positives / NULLIF(total_alerts, 0), 0) AS false_positive_ratio,
                COALESCE(high_risk_alerts / NULLIF(total_alerts, 0), 0) AS high_risk_jurisdiction_coverage,
                0.0::double precision AS policy_override_frequency
            FROM base
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let mut out = std::collections::HashMap::new();
        out.insert(
            "sar_conversion_rate".to_string(),
            row.try_get("sar_conversion_rate")?,
        );
        out.insert(
            "alert_processing_time_hours".to_string(),
            row.try_get("alert_processing_time_hours")?,
        );
        out.insert(
            "false_positive_ratio".to_string(),
            row.try_get("false_positive_ratio")?,
        );
        out.insert(
            "high_risk_jurisdiction_coverage".to_string(),
            row.try_get("high_risk_jurisdiction_coverage")?,
        );
        out.insert("policy_override_frequency".to_string(), 0.0);

        Ok(out)
    }

    pub async fn sample_dismissed_alerts_for_qc(
        &self,
        sample_rate: f64,
        reviewer_id: &str,
    ) -> Result<i64, anyhow::Error> {
        let clamped_rate = sample_rate.clamp(0.05, 0.10);

        let sampled = sqlx::query_scalar(
            r#"
            WITH eligible AS (
                SELECT a.id
                FROM aml_cases a
                LEFT JOIN aml_alert_qc_reviews qc ON qc.aml_case_id = a.id
                WHERE a.status = 'Cleared'
                  AND a.updated_at >= NOW() - INTERVAL '30 days'
                  AND qc.aml_case_id IS NULL
            ),
            counts AS (
                SELECT COUNT(*)::int AS n FROM eligible
            ),
            picked AS (
                SELECT id
                FROM eligible
                ORDER BY random()
                LIMIT GREATEST(CEIL((SELECT n FROM counts) * $1)::int, 0)
            ),
            ins AS (
                INSERT INTO aml_alert_qc_reviews (aml_case_id, assigned_reviewer_id, sampled_at, review_status)
                SELECT id, $2, NOW(), 'pending'
                FROM picked
                ON CONFLICT (aml_case_id) DO NOTHING
                RETURNING aml_case_id
            )
            SELECT COUNT(*)::bigint FROM ins
            "#,
        )
        .bind(clamped_rate)
        .bind(reviewer_id)
        .fetch_one(&self.pool)
        .await
        .context("failed to create QC samples")?;

        Ok(sampled.unwrap_or(0))
    }

    pub async fn qc_stats(
        &self,
        filters: &MetricFilters,
    ) -> Result<AlertQualityControlStats, anyhow::Error> {
        let dismissed_alerts: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint
            FROM aml_cases a
            LEFT JOIN transactions t ON t.transaction_id = a.transaction_id
            WHERE a.status = 'Cleared'
              AND a.updated_at >= $1
              AND a.updated_at < $2
              AND ($3::text IS NULL OR t.metadata->>'corridor_id' = $3)
              AND ($4::text IS NULL OR t.metadata->>'user_tier' = $4)
              AND ($5::text IS NULL OR t.metadata->>'asset_class' = $5)
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .bind(filters.corridor_id.as_deref())
        .bind(filters.user_tier.as_deref())
        .bind(filters.asset_class.as_deref())
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*)::bigint AS sampled,
                COUNT(*) FILTER (WHERE review_status = 'pending')::bigint AS pending_reviews,
                COUNT(*) FILTER (WHERE review_outcome = 'missed_suspicious_activity')::bigint AS escalated
            FROM aml_alert_qc_reviews
            WHERE sampled_at >= $1
              AND sampled_at < $2
            "#,
        )
        .bind(filters.start_at)
        .bind(filters.end_at)
        .fetch_one(&self.pool)
        .await?;

        let sampled_alerts: i64 = row.try_get("sampled")?;
        let pending_reviews: i64 = row.try_get("pending_reviews")?;
        let escalated_findings: i64 = row.try_get("escalated")?;

        let sampling_rate = if dismissed_alerts > 0 {
            sampled_alerts as f64 / dismissed_alerts as f64
        } else {
            0.0
        };

        Ok(AlertQualityControlStats {
            dismissed_alerts,
            sampled_alerts,
            sampling_rate,
            pending_reviews,
            escalated_findings,
        })
    }

    pub async fn upsert_hourly_snapshot(
        &self,
        filters: &MetricFilters,
        kpis: &RealtimeComplianceKpis,
    ) -> Result<(), anyhow::Error> {
        let snapshot_at = Utc::now()
            .with_minute(0)
            .and_then(|v| v.with_second(0))
            .and_then(|v| v.with_nanosecond(0))
            .unwrap_or_else(Utc::now);

        sqlx::query(
            r#"
            INSERT INTO aml_effectiveness_metric_snapshots (
                snapshot_at,
                corridor_id,
                user_tier,
                rule_set,
                asset_class,
                total_alerts,
                sar_conversion_rate,
                alert_processing_time_hours,
                false_positive_ratio,
                high_risk_jurisdiction_coverage,
                policy_override_frequency
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10, $11
            )
            ON CONFLICT (snapshot_at, corridor_id, user_tier, rule_set, asset_class)
            DO UPDATE SET
                total_alerts = EXCLUDED.total_alerts,
                sar_conversion_rate = EXCLUDED.sar_conversion_rate,
                alert_processing_time_hours = EXCLUDED.alert_processing_time_hours,
                false_positive_ratio = EXCLUDED.false_positive_ratio,
                high_risk_jurisdiction_coverage = EXCLUDED.high_risk_jurisdiction_coverage,
                policy_override_frequency = EXCLUDED.policy_override_frequency,
                refreshed_at = NOW()
            "#,
        )
        .bind(snapshot_at)
        .bind(filters.corridor_id.as_deref().unwrap_or("all"))
        .bind(filters.user_tier.as_deref().unwrap_or("all"))
        .bind(filters.rule_set.as_deref().unwrap_or("all"))
        .bind(filters.asset_class.as_deref().unwrap_or("all"))
        .bind(kpis.total_alerts)
        .bind(kpis.sar_conversion_rate)
        .bind(kpis.alert_processing_time_hours)
        .bind(kpis.false_positive_ratio)
        .bind(kpis.high_risk_jurisdiction_coverage)
        .bind(kpis.policy_override_frequency)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn store_quarterly_report(
        &self,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        generated_by: &str,
        report: &QuarterlyEffectivenessReport,
    ) -> Result<(), anyhow::Error> {
        let kpi_payload = serde_json::to_value(&report.kpis)?;
        let policy_payload = serde_json::to_value(&report.policy_effectiveness)?;
        let heatmap_payload = serde_json::to_value(&report.heatmap)?;
        let benchmark_payload = serde_json::to_value(&report.benchmarking)?;
        let trend_payload = serde_json::to_value(&report.sar_trend)?;

        sqlx::query(
            r#"
            INSERT INTO compliance_effectiveness_reports (
                id,
                report_type,
                period_start,
                period_end,
                total_alerts,
                false_positives,
                false_positive_rate,
                avg_resolution_time_hrs,
                generated_by,
                format,
                kpi_payload,
                policy_effectiveness_payload,
                heatmap_payload,
                benchmark_payload,
                trend_alerts_payload,
                policy_adjustments,
                generated_at
            ) VALUES (
                $1, 'quarterly', $2, $3,
                $4, $5, $6, $7,
                $8, 'json',
                $9, $10, $11, $12, $13, $14,
                NOW()
            )
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(report.id)
        .bind(period_start)
        .bind(period_end)
        .bind(report.kpis.total_alerts as i32)
        .bind((report.kpis.false_positive_ratio * report.kpis.total_alerts as f64).round() as i32)
        .bind(report.kpis.false_positive_ratio)
        .bind(report.kpis.alert_processing_time_hours)
        .bind(generated_by)
        .bind(kpi_payload)
        .bind(policy_payload)
        .bind(heatmap_payload)
        .bind(benchmark_payload)
        .bind(trend_payload)
        .bind(report.policy_adjustments.clone())
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO compliance_report_audit (report_id, action, actor_id, actor_role, metadata)
            VALUES ($1, 'generated', $2, 'system', $3)
            "#,
        )
        .bind(report.id)
        .bind(generated_by)
        .bind(serde_json::json!({"report_type": "quarterly"}))
        .execute(&self.pool)
        .await
        .ok();

        Ok(())
    }

    pub async fn latest_quarterly_report(
        &self,
    ) -> Result<Option<QuarterlyEffectivenessReport>, anyhow::Error> {
        let row = sqlx::query(
            r#"
            SELECT id, period_start, period_end, generated_at,
                   kpi_payload,
                   policy_effectiveness_payload,
                   heatmap_payload,
                   benchmark_payload,
                   trend_alerts_payload,
                   policy_adjustments
            FROM compliance_effectiveness_reports
            WHERE report_type = 'quarterly'
            ORDER BY period_end DESC, generated_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let kpis: RealtimeComplianceKpis = serde_json::from_value(
            row.try_get::<Value, _>("kpi_payload")
                .unwrap_or_else(|_| serde_json::json!({})),
        )
        .unwrap_or(RealtimeComplianceKpis {
            total_alerts: 0,
            sar_conversion_rate: 0.0,
            alert_processing_time_hours: 0.0,
            false_positive_ratio: 0.0,
            high_risk_jurisdiction_coverage: 0.0,
            policy_override_frequency: 0.0,
            refreshed_at: Utc::now(),
        });

        let policy_effectiveness: Vec<PolicyEffectivenessMetric> = serde_json::from_value(
            row.try_get::<Value, _>("policy_effectiveness_payload")
                .unwrap_or_else(|_| serde_json::json!([])),
        )
        .unwrap_or_default();

        let heatmap: Vec<RiskHeatmapCell> = serde_json::from_value(
            row.try_get::<Value, _>("heatmap_payload")
                .unwrap_or_else(|_| serde_json::json!([])),
        )
        .unwrap_or_default();

        let benchmarking: Vec<BenchmarkComparison> = serde_json::from_value(
            row.try_get::<Value, _>("benchmark_payload")
                .unwrap_or_else(|_| serde_json::json!([])),
        )
        .unwrap_or_default();

        let sar_trend: Vec<SarTrendPoint> = serde_json::from_value(
            row.try_get::<Value, _>("trend_alerts_payload")
                .unwrap_or_else(|_| serde_json::json!([])),
        )
        .unwrap_or_default();

        let policy_adjustments: Vec<String> = row.try_get("policy_adjustments").unwrap_or_default();

        Ok(Some(QuarterlyEffectivenessReport {
            id: row.try_get("id")?,
            period_start: row.try_get("period_start")?,
            period_end: row.try_get("period_end")?,
            generated_at: row.try_get("generated_at")?,
            kpis,
            policy_effectiveness,
            heatmap,
            benchmarking,
            sar_trend,
            policy_adjustments,
        }))
    }

    pub async fn quarterly_report_exists(
        &self,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<bool, anyhow::Error> {
        let exists = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM compliance_effectiveness_reports
                WHERE report_type = 'quarterly'
                  AND period_start = $1
                  AND period_end = $2
            )
            "#,
        )
        .bind(period_start)
        .bind(period_end)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists.unwrap_or(false))
    }
}
