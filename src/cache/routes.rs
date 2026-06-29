use crate::cache::handlers::{get_cache_stats, purge_cache, CacheAdminState};
use crate::middleware::rbac::extract_identity;
use axum::{middleware, routing::{get, post}, Router};
use std::sync::Arc;

pub fn cache_admin_router(state: Arc<CacheAdminState>) -> Router {
    Router::new()
        .route("/api/admin/infra/cache/purge", post(purge_cache))
        .route("/api/admin/infra/cache/stats", get(get_cache_stats))
        .route_layer(middleware::from_fn(extract_identity))
        .with_state(state)
}
