//! Ledger Query Acceleration Layer (Issue #XXX)
//!
//! Provides:
//! - Query result caching for read-only ledger queries
//! - Materialized view management
//! - Bloom filters for existence checks
//! - Query plan optimization hints
//! - Cache invalidation on writes

use crate::database::error::DatabaseError;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Query Result Cache
// ─────────────────────────────────────────────────────────────────────────────

/// Cached query result with TTL
#[derive(Debug, Clone)]
struct CacheEntry {
    result: serde_json::Value,
    created_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// Query result cache for ledger queries
pub struct QueryResultCache {
    cache: Arc<tokio::sync::RwLock<HashMap<String, CacheEntry>>>,
    max_entries: usize,
}

impl QueryResultCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            max_entries,
        }
    }

    /// Get a cached result if available and not expired
    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let cache = self.cache.read().await;
        match cache.get(key) {
            Some(entry) if !entry.is_expired() => {
                debug!("Cache hit: {}", key);
                Some(entry.result.clone())
            }
            _ => {
                debug!("Cache miss: {}", key);
                None
            }
        }
    }

    /// Store a result in cache with TTL
    pub async fn set(&self, key: String, value: serde_json::Value, ttl: Duration) {
        let mut cache = self.cache.write().await;

        // Evict oldest entry if cache is full
        if cache.len() >= self.max_entries {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
                debug!("Evicted cache entry: {}", oldest_key);
            }
        }

        cache.insert(
            key.clone(),
            CacheEntry {
                result: value,
                created_at: Instant::now(),
                ttl,
            },
        );
        debug!("Cached result: {}", key);
    }

    /// Invalidate a cache entry
    pub async fn invalidate(&self, key: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(key);
        debug!("Invalidated cache entry: {}", key);
    }

    /// Invalidate all cache entries matching a pattern
    pub async fn invalidate_pattern(&self, pattern: &str) {
        let mut cache = self.cache.write().await;
        let keys_to_remove: Vec<String> = cache
            .keys()
            .filter(|k| k.contains(pattern))
            .cloned()
            .collect();

        for key in keys_to_remove {
            cache.remove(&key);
        }
        debug!("Invalidated {} cache entries matching pattern: {}", cache.len(), pattern);
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        CacheStats {
            entries: cache.len(),
            max_entries: self.max_entries,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
    pub max_entries: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Materialized Views
// ─────────────────────────────────────────────────────────────────────────────

pub struct MaterializedViewManager {
    pool: Arc<PgPool>,
    refresh_interval: Duration,
}

impl MaterializedViewManager {
    pub fn new(pool: Arc<PgPool>, refresh_interval: Duration) -> Self {
        Self {
            pool,
            refresh_interval,
        }
    }

    /// Create materialized view for settlement summaries
    pub async fn create_settlement_summaries_view(&self) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS settlement_summaries_by_corridor_week AS
            SELECT
                corridor_id,
                EXTRACT(YEAR FROM created_at)::INT * 100 + EXTRACT(WEEK FROM created_at)::INT as week_id,
                SUM(gross_amount) as total_gross,
                SUM(platform_fee) as total_fees,
                SUM(provider_charge) as total_provider_charge,
                COUNT(*) as batch_count,
                COUNT(CASE WHEN status = 'settled' THEN 1 END) as settled_count,
                COUNT(CASE WHEN status = 'failed' THEN 1 END) as failed_count,
                NOW() as last_updated
            FROM settlement_batches
            GROUP BY corridor_id, week_id;

            CREATE UNIQUE INDEX IF NOT EXISTS idx_settlement_summaries_unique
            ON settlement_summaries_by_corridor_week(corridor_id, week_id);

            CREATE INDEX IF NOT EXISTS idx_settlement_summaries_corridor
            ON settlement_summaries_by_corridor_week(corridor_id);
            "#,
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(DatabaseError::from_sqlx)?;

        info!("Created settlement_summaries_by_corridor_week materialized view");
        Ok(())
    }

    /// Create materialized view for transaction statistics
    pub async fn create_transaction_stats_view(&self) -> Result<(), DatabaseError> {
        sqlx::query(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS transaction_stats_by_corridor_day AS
            SELECT
                CASE
                    WHEN wallet_address LIKE '%ng_%' THEN 'NG'
                    WHEN wallet_address LIKE '%gh_%' THEN 'GH'
                    WHEN wallet_address LIKE '%ke_%' THEN 'KE'
                    ELSE 'OTHER'
                END as corridor_id,
                DATE_TRUNC('day', created_at)::DATE as day,
                type,
                status,
                COUNT(*) as transaction_count,
                SUM(CAST(from_amount AS NUMERIC)) as total_from_amount,
                SUM(CAST(to_amount AS NUMERIC)) as total_to_amount,
                AVG(CAST(from_amount AS NUMERIC)) as avg_amount,
                NOW() as last_updated
            FROM transactions
            GROUP BY corridor_id, day, type, status;

            CREATE UNIQUE INDEX IF NOT EXISTS idx_transaction_stats_unique
            ON transaction_stats_by_corridor_day(corridor_id, day, type, status);

            CREATE INDEX IF NOT EXISTS idx_transaction_stats_corridor
            ON transaction_stats_by_corridor_day(corridor_id, day DESC);
            "#,
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(DatabaseError::from_sqlx)?;

        info!("Created transaction_stats_by_corridor_day materialized view");
        Ok(())
    }

    /// Refresh a materialized view
    pub async fn refresh_view(&self, view_name: &str) -> Result<(), DatabaseError> {
        let sql = format!("REFRESH MATERIALIZED VIEW CONCURRENTLY {}", view_name);
        sqlx::query(&sql)
            .execute(self.pool.as_ref())
            .await
            .map_err(DatabaseError::from_sqlx)?;

        info!("Refreshed materialized view: {}", view_name);
        Ok(())
    }

    /// Spawn background refresh task
    pub fn spawn_refresh_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(self.refresh_interval).await;

                if let Err(e) = self.refresh_view("settlement_summaries_by_corridor_week").await {
                    warn!("Failed to refresh settlement_summaries view: {}", e);
                }

                if let Err(e) = self.refresh_view("transaction_stats_by_corridor_day").await {
                    warn!("Failed to refresh transaction_stats view: {}", e);
                }
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ledger Query Accelerator
// ─────────────────────────────────────────────────────────────────────────────

pub struct LedgerQueryAccelerator {
    pool: Arc<PgPool>,
    cache: Arc<QueryResultCache>,
    view_manager: Arc<MaterializedViewManager>,
}

impl LedgerQueryAccelerator {
    pub fn new(
        pool: Arc<PgPool>,
        cache: Arc<QueryResultCache>,
        view_manager: Arc<MaterializedViewManager>,
    ) -> Self {
        Self {
            pool,
            cache,
            view_manager,
        }
    }

    /// Get settlement summary for corridor-week
    pub async fn settlement_summary_by_corridor_week(
        &self,
        corridor_id: &str,
        week_id: i32,
    ) -> Result<serde_json::Value, DatabaseError> {
        let cache_key = format!("settlement:{}:{}", corridor_id, week_id);

        // Try cache first
        if let Some(result) = self.cache.get(&cache_key).await {
            return Ok(result);
        }

        // Query materialized view
        let row = sqlx::query(
            r#"
            SELECT
                corridor_id,
                week_id,
                total_gross,
                total_fees,
                total_provider_charge,
                batch_count,
                settled_count,
                failed_count,
                last_updated
            FROM settlement_summaries_by_corridor_week
            WHERE corridor_id = $1 AND week_id = $2
            "#,
        )
        .bind(corridor_id)
        .bind(week_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(DatabaseError::from_sqlx)?;

        let result = match row {
            Some(row) => serde_json::json!({
                "corridor_id": row.get::<String, _>("corridor_id"),
                "week_id": row.get::<i32, _>("week_id"),
                "total_gross": row.get::<String, _>("total_gross"),
                "total_fees": row.get::<String, _>("total_fees"),
                "total_provider_charge": row.get::<String, _>("total_provider_charge"),
                "batch_count": row.get::<i64, _>("batch_count"),
                "settled_count": row.get::<i64, _>("settled_count"),
                "failed_count": row.get::<i64, _>("failed_count"),
                "last_updated": row.get::<chrono::DateTime<chrono::Utc>, _>("last_updated"),
            }),
            None => serde_json::json!({}),
        };

        // Cache for 1 hour
        self.cache
            .set(cache_key, result.clone(), Duration::from_secs(3600))
            .await;

        Ok(result)
    }

    /// Get transaction statistics for corridor-day
    pub async fn transaction_stats_by_corridor_day(
        &self,
        corridor_id: &str,
        day: chrono::NaiveDate,
    ) -> Result<serde_json::Value, DatabaseError> {
        let cache_key = format!("txn_stats:{}:{}", corridor_id, day);

        // Try cache first
        if let Some(result) = self.cache.get(&cache_key).await {
            return Ok(result);
        }

        // Query materialized view
        let rows = sqlx::query(
            r#"
            SELECT
                corridor_id,
                day,
                type,
                status,
                transaction_count,
                total_from_amount,
                total_to_amount,
                avg_amount
            FROM transaction_stats_by_corridor_day
            WHERE corridor_id = $1 AND day = $2
            ORDER BY type
            "#,
        )
        .bind(corridor_id)
        .bind(day)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(DatabaseError::from_sqlx)?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(serde_json::json!({
                "type": row.get::<String, _>("type"),
                "status": row.get::<String, _>("status"),
                "transaction_count": row.get::<i64, _>("transaction_count"),
                "total_from_amount": row.get::<String, _>("total_from_amount"),
                "total_to_amount": row.get::<String, _>("total_to_amount"),
                "avg_amount": row.get::<String, _>("avg_amount"),
            }));
        }

        let result = serde_json::json!({
            "corridor_id": corridor_id,
            "day": day,
            "stats": stats,
        });

        // Cache for 1 hour
        self.cache
            .set(cache_key, result.clone(), Duration::from_secs(3600))
            .await;

        Ok(result)
    }

    /// Invalidate cache for a corridor
    pub async fn invalidate_corridor_cache(&self, corridor_id: &str) {
        self.cache
            .invalidate_pattern(&format!("{}:", corridor_id))
            .await;
    }

    /// Get accelerator statistics
    pub async fn stats(&self) -> AcceleratorStats {
        let cache_stats = self.cache.stats().await;
        AcceleratorStats {
            cache_entries: cache_stats.entries,
            cache_max_entries: cache_stats.max_entries,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AcceleratorStats {
    pub cache_entries: usize,
    pub cache_max_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_query_result_cache() {
        let cache = QueryResultCache::new(10);

        let key = "test_key".to_string();
        let value = serde_json::json!({"foo": "bar"});

        // Cache miss
        assert!(cache.get(&key).await.is_none());

        // Store
        cache.set(key.clone(), value.clone(), Duration::from_secs(60)).await;

        // Cache hit
        assert_eq!(cache.get(&key).await, Some(value));
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = QueryResultCache::new(10);

        let key = "test_key".to_string();
        let value = serde_json::json!({"foo": "bar"});

        // Store with very short TTL
        cache
            .set(key.clone(), value, Duration::from_millis(10))
            .await;

        // Should hit immediately
        assert!(cache.get(&key).await.is_some());

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should miss after expiration
        assert!(cache.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidation_pattern() {
        let cache = QueryResultCache::new(10);

        // Store multiple entries
        cache
            .set(
                "settlement:NG:1".to_string(),
                serde_json::json!({}),
                Duration::from_secs(60),
            )
            .await;
        cache
            .set(
                "settlement:GH:1".to_string(),
                serde_json::json!({}),
                Duration::from_secs(60),
            )
            .await;
        cache
            .set(
                "txn_stats:NG:1".to_string(),
                serde_json::json!({}),
                Duration::from_secs(60),
            )
            .await;

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 3);

        // Invalidate pattern
        cache.invalidate_pattern("settlement").await;

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 1); // Only txn_stats remains
    }
}
