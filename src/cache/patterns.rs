//! Cache-Aside helpers (Issue #459)
//!
//! Thin wrappers over `MultiLevelCache::l2_get_or_rebuild`. Do not add a
//! third cache-aside implementation — delegate to the existing single-flight
//! rebuild path that already provides stampede protection.
//!
//! Three datasets are new; wallet balances and exchange rates already have
//! their own repository-level caching and are not duplicated here.

use crate::cache::multi_level::MultiLevelCache;
use serde::{de::DeserializeOwned, Serialize};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, instrument};

/// Thin facade over `MultiLevelCache::l2_get_or_rebuild` with structured
/// cache_hit / cache_miss log fields.
pub struct CachingRepository {
    cache: Arc<MultiLevelCache>,
}

impl CachingRepository {
    pub fn new(cache: Arc<MultiLevelCache>) -> Self {
        Self { cache }
    }

    /// Get a value from L2, or rebuild via `fetch_fn` under single-flight guard.
    ///
    /// Emits `{ cache_hit, namespace, key }` at DEBUG level on every call.
    pub async fn get_or_fetch<T, F, Fut>(
        &self,
        namespace: &str,
        key: &str,
        ttl: Duration,
        fetch_fn: F,
    ) -> Result<T, String>
    where
        T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        // Fast path: L2 hit
        if let Some(v) = self.cache.l2_get::<T>(namespace, key).await {
            debug!(cache_hit = true, namespace, key, "Cache-aside: L2 hit");
            return Ok(v);
        }

        // Slow path: single-flight rebuild (at most 1 DB call per key)
        debug!(cache_hit = false, namespace, key, "Cache-aside: L2 miss → rebuild");
        self.cache
            .l2_get_or_rebuild(namespace, key, ttl, fetch_fn)
            .await
    }

    /// Invalidate a key from L2 (called after write).
    pub async fn invalidate(&self, key: &str) {
        self.cache.l2_invalidate::<serde_json::Value>(key).await;
        debug!(key, "Cache-aside: key invalidated");
    }
}

// ---------------------------------------------------------------------------
// TTL constants for the three new datasets
// ---------------------------------------------------------------------------

pub const TTL_USER_PROFILE: Duration    = Duration::from_secs(300);   // 5 min
pub const TTL_USER_ONBOARDING: Duration = Duration::from_secs(600);   // 10 min
pub const TTL_PARTNER_CONFIG: Duration  = Duration::from_secs(1800);  // 30 min
pub const TTL_PARTNER_LIQUIDITY: Duration = Duration::from_secs(30);  // 30 s — high volatility
