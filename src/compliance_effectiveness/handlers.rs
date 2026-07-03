use crate::compliance_effectiveness::models::{
    MetricFilterQuery, MetricFilters, QcSamplingRequest, QcSamplingResponse, QuarterlyReportRequest,
};
use crate::compliance_effectiveness::repository::ComplianceEffectivenessRepository;
use crate::compliance_effectiveness::service::ReportGenerationService;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct ComplianceEffectivenessState {
    pub service: Arc<ReportGenerationService>,
    pub repo: Arc<ComplianceEffectivenessRepository>,
}

pub fn compliance_effectiveness_routes(state: Arc<ComplianceEffectivenessState>) -> Router {
    Router::new()
        .route("/api/admin/compliance/effectiveness/kpis", get(get_kpis))
        .route(
            "/api/admin/compliance/effectiveness/heatmap",
            get(get_heatmap),
        )
        .route(
            "/api/admin/compliance/effectiveness/policy-effectiveness",
            get(get_policy_effectiveness),
        )
        .route(
            "/api/admin/compliance/effectiveness/benchmarking",
            get(get_benchmarking),
        )
        .route(
            "/api/admin/compliance/effectiveness/sar-trend",
            get(get_sar_trend),
        )
        .route("/api/admin/compliance/effectiveness/qc", get(get_qc_stats))
        .route(
            "/api/admin/compliance/effectiveness/qc/sample",
            post(run_qc_sampling),
        )
        .route(
            "/api/admin/compliance/effectiveness/reports/quarterly/generate",
            post(generate_quarterly_report),
        )
        .route(
            "/api/admin/compliance/effectiveness/reports/quarterly/latest",
            get(get_latest_quarterly_report),
        )
        .with_state(state)
}

pub async fn get_kpis(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Query(query): Query<MetricFilterQuery>,
) -> impl IntoResponse {
    let filters = MetricFilters::from_query(query);
    match state.service.dashboard_kpis(&filters).await {
        Ok(kpis) => (StatusCode::OK, Json(kpis)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_heatmap(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Query(query): Query<MetricFilterQuery>,
) -> impl IntoResponse {
    let filters = MetricFilters::from_query(query);
    match state.repo.risk_heatmap(&filters).await {
        Ok(heatmap) => (StatusCode::OK, Json(heatmap)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_policy_effectiveness(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Query(query): Query<MetricFilterQuery>,
) -> impl IntoResponse {
    let filters = MetricFilters::from_query(query);
    match state.repo.policy_effectiveness(&filters, 25).await {
        Ok(metrics) => (StatusCode::OK, Json(metrics)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_benchmarking(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Query(query): Query<MetricFilterQuery>,
) -> impl IntoResponse {
    let filters = MetricFilters::from_query(query);
    match state.service.dashboard_kpis(&filters).await {
        Ok(kpis) => match state.repo.benchmark_comparisons(&kpis).await {
            Ok(benchmarks) => (StatusCode::OK, Json(benchmarks)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_sar_trend(
    State(state): State<Arc<ComplianceEffectivenessState>>,
) -> impl IntoResponse {
    match state.repo.sar_conversion_trend(18).await {
        Ok(trend) => {
            let has_alert = trend.last().map(|t| t.deviation_alert).unwrap_or(false);
            (
                StatusCode::OK,
                Json(json!({
                    "trend": trend,
                    "latest_deviation_alert": has_alert,
                    "moving_average_window_months": 6
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_qc_stats(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Query(query): Query<MetricFilterQuery>,
) -> impl IntoResponse {
    let filters = MetricFilters::from_query(query);
    match state.repo.qc_stats(&filters).await {
        Ok(stats) => (StatusCode::OK, Json(stats)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn run_qc_sampling(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Json(payload): Json<QcSamplingRequest>,
) -> impl IntoResponse {
    let sample_rate = payload.sample_rate.unwrap_or(0.05).clamp(0.05, 0.10);
    let reviewer = payload
        .senior_reviewer_id
        .unwrap_or_else(|| "senior_compliance_queue".to_string());

    match state
        .repo
        .sample_dismissed_alerts_for_qc(sample_rate, &reviewer)
        .await
    {
        Ok(sampled_count) => (
            StatusCode::OK,
            Json(QcSamplingResponse {
                sampled_count,
                sample_rate_applied: sample_rate,
                assigned_reviewer_id: reviewer,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn generate_quarterly_report(
    State(state): State<Arc<ComplianceEffectivenessState>>,
    Json(payload): Json<QuarterlyReportRequest>,
) -> impl IntoResponse {
    let generated_by = payload.generated_by.unwrap_or_else(|| "system".to_string());
    match state.service.generate_quarterly_report(&generated_by).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_latest_quarterly_report(
    State(state): State<Arc<ComplianceEffectivenessState>>,
) -> impl IntoResponse {
    match state.repo.latest_quarterly_report().await {
        Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "report not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
