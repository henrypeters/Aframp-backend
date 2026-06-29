use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct MetricFilterQuery {
    pub corridor_id: Option<String>,
    pub user_tier: Option<String>,
    pub rule_set: Option<String>,
    pub asset_class: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MetricFilters {
    pub corridor_id: Option<String>,
    pub user_tier: Option<String>,
    pub rule_set: Option<String>,
    pub asset_class: Option<String>,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
}

impl MetricFilters {
    pub fn from_query(q: MetricFilterQuery) -> Self {
        let now = Utc::now();
        Self {
            corridor_id: q.corridor_id,
            user_tier: q.user_tier,
            rule_set: q.rule_set,
            asset_class: q.asset_class,
            start_at: q
                .start_at
                .unwrap_or_else(|| now - chrono::Duration::days(30)),
            end_at: q.end_at.unwrap_or(now),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeComplianceKpis {
    pub total_alerts: i64,
    pub sar_conversion_rate: f64,
    pub alert_processing_time_hours: f64,
    pub false_positive_ratio: f64,
    pub high_risk_jurisdiction_coverage: f64,
    pub policy_override_frequency: f64,
    pub refreshed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEffectivenessMetric {
    pub rule_name: String,
    pub total_alerts: i64,
    pub sar_conversions: i64,
    pub false_positive_ratio: f64,
    pub noise_index: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskHeatmapCell {
    pub corridor_id: String,
    pub user_tier: String,
    pub asset_class: String,
    pub transaction_volume: f64,
    pub high_risk_alerts: i64,
    pub risk_intensity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub metric_name: String,
    pub current_value: f64,
    pub internal_baseline: f64,
    pub industry_benchmark: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SarTrendPoint {
    pub month: NaiveDate,
    pub conversion_rate: f64,
    pub moving_average_6m: Option<f64>,
    pub deviation_ratio: Option<f64>,
    pub deviation_alert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertQualityControlStats {
    pub dismissed_alerts: i64,
    pub sampled_alerts: i64,
    pub sampling_rate: f64,
    pub pending_reviews: i64,
    pub escalated_findings: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QcSamplingRequest {
    pub sample_rate: Option<f64>,
    pub senior_reviewer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcSamplingResponse {
    pub sampled_count: i64,
    pub sample_rate_applied: f64,
    pub assigned_reviewer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarterlyEffectivenessReport {
    pub id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub generated_at: DateTime<Utc>,
    pub kpis: RealtimeComplianceKpis,
    pub policy_effectiveness: Vec<PolicyEffectivenessMetric>,
    pub heatmap: Vec<RiskHeatmapCell>,
    pub benchmarking: Vec<BenchmarkComparison>,
    pub sar_trend: Vec<SarTrendPoint>,
    pub policy_adjustments: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuarterlyReportRequest {
    pub generated_by: Option<String>,
}

pub fn current_quarter_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let quarter_start_month = match now.month() {
        1..=3 => 1,
        4..=6 => 4,
        7..=9 => 7,
        _ => 10,
    };

    let start = Utc
        .with_ymd_and_hms(now.year(), quarter_start_month, 1, 0, 0, 0)
        .single()
        .unwrap_or(now);

    let end = if quarter_start_month == 10 {
        Utc.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
            .single()
            .unwrap_or(now)
    } else {
        Utc.with_ymd_and_hms(now.year(), quarter_start_month + 3, 1, 0, 0, 0)
            .single()
            .unwrap_or(now)
    };

    (start, end)
}

pub fn previous_quarter_bounds(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let (current_start, _) = current_quarter_bounds(now);
    let previous_end = current_start;
    let prev_ref = current_start - chrono::Duration::days(1);
    let (previous_start, _) = current_quarter_bounds(prev_ref);
    (previous_start, previous_end)
}
