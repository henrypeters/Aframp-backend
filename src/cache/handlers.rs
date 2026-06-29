//! Cache admin HTTP handlers (Issue #459)
//!
//! POST /api/admin/infra/cache/purge — clear keys by pattern
//! GET  /api/admin/infra/cache/stats — L1/L2/Redis memory summary
//!
//! Both endpoints require X-User-Role (compliance_officer or admin) via
//! `extract_identity` middleware in routes.rs.

use crate::cache::cache::{Cache as CacheTrait, RedisCache};
use crate::cache::metrics::{CacheSizeMetrics, L2Metrics};
use crate::cache::multi_level::MultiLevelCache;
use crate::middleware::rbac::{CallerIdentity, ROLE_COMPLIANCE_OFFICER};
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CacheAdminState {
    pub multi_cache: Arc<MultiLevelCache>,
    pub redis: Arc<RedisCache>,
    pub pool: Arc<PgPool>,
    pub l2_metrics: Arc<L2Metrics>,
    pub size_metrics: Arc<CacheSizeMetrics>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PurgeCacheRequest {
    /// Redis key namespace to target (e.g. "v1:rate", "v1:user", "v1:partner")
    pub namespace: String,
    /// Optional fine-grained pattern; defaults to `<namespace>:*`
    pub pattern: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PurgeCacheResponse {
    pub keys_deleted: u64,
    pub namespace: String,
    pub pattern: String,
}

#[derive(Debug, Serialize)]
pub struct CacheStatsResponse {
    pub l1: L1StatsSnapshot,
    pub l2: L2StatsSnapshot,
    pub redis: RedisMemorySnapshot,
    pub pool: PoolStatsSnapshot,
}

#[derive(Debug, Serialize)]
pub struct L1StatsSnapshot {
    pub fee_structures_entries: u64,
    pub currency_configs_entries: u64,
    pub provider_lists_entries: u64,
}

#[derive(Debug, Serialize)]
pub struct L2StatsSnapshot {
    pub hits_total: u64,
    pub misses_total: u64,
    pub hit_ratio_5m: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct RedisMemorySnapshot {
    pub used_memory_mb: f64,
    pub maxmemory_mb: Option<f64>,
    pub memory_usage_pct: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PoolStatsSnapshot {
    pub connections: u32,
    pub idle: u32,
    pub in_use: u32,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/admin/infra/cache/purge
pub async fn purge_cache(
    State(state): State<Arc<CacheAdminState>>,
    Extension(caller): Extension<CallerIdentity>,
    Json(req): Json<PurgeCacheRequest>,
) -> impl IntoResponse {
    // Require compliance_officer or admin role
    if caller.role != ROLE_COMPLIANCE_OFFICER && caller.role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "admin or compliance_officer role required" })),
        )
            .into_response();
    }

    let pattern = req
        .pattern
        .clone()
        .unwrap_or_else(|| format!("{}:*", req.namespace));

    info!(
        namespace = %req.namespace,
        pattern = %pattern,
        initiator = %caller.user_id,
        reason = ?req.reason,
        "Cache purge requested"
    );

    // SCAN-based delete
    let keys_deleted = match CacheTrait::<serde_json::Value>::delete_pattern(
        &*state.redis,
        &pattern,
    )
    .await
    {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "Cache purge failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("purge failed: {}", e) })),
            )
                .into_response();
        }
    };

    // Also invalidate L1 for known namespaces
    if req.namespace.contains("fee") {
        state
            .multi_cache
            .l1_invalidate_all(crate::cache::l1::L1Category::FeeStructures)
            .await;
    }
    if req.namespace.contains("rate") {
        state
            .multi_cache
            .l1_invalidate_all(crate::cache::l1::L1Category::CurrencyConfigs)
            .await;
    }

    // Write audit row
    if let Ok(initiator_uuid) = Uuid::parse_str(&caller.user_id) {
        let _ = sqlx::query(
            r#"INSERT INTO cache_invalidation_logs
               (initiator_id, initiator_role, target_namespace, pattern_used, keys_deleted, reason)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(initiator_uuid)
        .bind(&caller.role)
        .bind(&req.namespace)
        .bind(&pattern)
        .bind(keys_deleted as i64)
        .bind(req.reason.as_deref())
        .execute(state.pool.as_ref())
        .await;
    }

    info!(keys_deleted, pattern = %pattern, "Cache purge complete");

    (
        StatusCode::OK,
        Json(json!(PurgeCacheResponse {
            keys_deleted,
            namespace: req.namespace,
            pattern,
        })),
    )
        .into_response()
}

/// GET /api/admin/infra/cache/stats
pub async fn get_cache_stats(
    State(state): State<Arc<CacheAdminState>>,
    Extension(caller): Extension<CallerIdentity>,
) -> impl IntoResponse {
    if caller.role != ROLE_COMPLIANCE_OFFICER && caller.role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "admin or compliance_officer role required" })),
        )
            .into_response();
    }

    // L1 entry counts
    let l1 = L1StatsSnapshot {
        fee_structures_entries: state.multi_cache.l1.fee_structures.entry_count(),
        currency_configs_entries: state.multi_cache.l1.currency_configs.entry_count(),
        provider_lists_entries: state.multi_cache.l1.provider_lists.entry_count(),
    };

    // L2 aggregate counters (read from in-process state via Arc<L2Metrics>)
    let l2 = L2StatsSnapshot {
        hits_total: state.l2_metrics.hits_count("all"),
        misses_total: state.l2_metrics.misses_count("all"),
        hit_ratio_5m: state.l2_metrics.rolling_hit_ratio_5m(),
    };

    // Redis INFO memory
    let redis_mem = fetch_redis_memory(&state.redis).await;

    // bb8 pool stats
    let pool_stats = {
        let s = state.redis.pool.state();
        PoolStatsSnapshot {
            connections: s.connections as u32,
            idle: s.idle_connections as u32,
            in_use: (s.connections - s.idle_connections) as u32,
        }
    };

    (
        StatusCode::OK,
        Json(json!(CacheStatsResponse {
            l1,
            l2,
            redis: redis_mem,
            pool: pool_stats,
        })),
    )
        .into_response()
}

async fn fetch_redis_memory(redis: &Arc<RedisCache>) -> RedisMemorySnapshot {
    let mut conn = match redis.get_connection().await {
        Ok(c) => c,
        Err(_) => {
            return RedisMemorySnapshot {
                used_memory_mb: 0.0,
                maxmemory_mb: None,
                memory_usage_pct: None,
            }
        }
    };

    let info: String = match redis::cmd("INFO")
        .arg("memory")
        .query_async(&mut *conn)
        .await
    {
        Ok(s) => s,
        Err(_) => {
            return RedisMemorySnapshot {
                used_memory_mb: 0.0,
                maxmemory_mb: None,
                memory_usage_pct: None,
            }
        }
    };

    let used = parse_info_field(&info, "used_memory:");
    let maxmem = parse_info_field(&info, "maxmemory:");

    let used_mb = used.unwrap_or(0) as f64 / 1_048_576.0;
    let max_mb = maxmem.map(|m| m as f64 / 1_048_576.0);
    let pct = max_mb.and_then(|m| if m > 0.0 { Some(used_mb / m * 100.0) } else { None });

    RedisMemorySnapshot {
        used_memory_mb: used_mb,
        maxmemory_mb: max_mb,
        memory_usage_pct: pct,
    }
}

fn parse_info_field(info: &str, field: &str) -> Option<u64> {
    info.lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<u64>().ok())
}
