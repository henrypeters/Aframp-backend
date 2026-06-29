//! Cache invalidation pipeline (Issue #459)
//!
//! All cache deletions on write paths are routed through this module so key
//! mapping is in one place. Repos call `pipeline.process(event)` instead of
//! doing inline `cache.delete(...)` calls. That eliminates drift and
//! double-delete bugs when key format changes.
//!
//! The pipeline also writes to `cache_invalidation_logs` when reason = "write_through".

use crate::cache::cache::{Cache as CacheTrait, RedisCache};
use crate::cache::keys::{
    exchange_rate::CurrencyPairKey, partner::{ConfigKey, LiquidityKey},
    user::{OnboardingKey, ProfileKey}, wallet::BalanceKey,
};
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Events that trigger cache invalidation.
#[derive(Debug, Clone)]
pub enum InvalidationEvent {
    WalletBalanceChanged { address: String },
    ExchangeRateUpdated { from: String, to: String },
    UserProfileUpdated { user_id: Uuid },
    UserOnboardingUpdated { user_id: Uuid },
    FeeStructureUpdated,
    PartnerConfigUpdated { partner_id: Uuid },
    PartnerLiquidityUpdated { partner_id: Uuid },
}

pub struct InvalidationPipeline {
    redis: Arc<RedisCache>,
    pool: Option<Arc<PgPool>>,
}

impl InvalidationPipeline {
    pub fn new(redis: Arc<RedisCache>, pool: Option<Arc<PgPool>>) -> Arc<Self> {
        Arc::new(Self { redis, pool })
    }

    /// Process an invalidation event — deletes the relevant cache key(s)
    /// and writes an audit row if `pool` is available.
    pub async fn process(&self, event: InvalidationEvent) {
        match &event {
            InvalidationEvent::WalletBalanceChanged { address } => {
                let key = BalanceKey::new(address).to_string();
                self.delete_key(&key, "v1:wallet").await;
            }

            InvalidationEvent::ExchangeRateUpdated { from, to } => {
                let key = CurrencyPairKey::new(from, to).to_string();
                self.delete_key(&key, "v1:rate").await;
            }

            InvalidationEvent::UserProfileUpdated { user_id } => {
                let key = ProfileKey::new(user_id.to_string()).to_string();
                self.delete_key(&key, "v1:user").await;
            }

            InvalidationEvent::UserOnboardingUpdated { user_id } => {
                let key = OnboardingKey::new(user_id.to_string()).to_string();
                self.delete_key(&key, "v1:user").await;
            }

            InvalidationEvent::FeeStructureUpdated => {
                // Pattern delete all fee structure keys
                self.delete_pattern("v1:fee:*", "v1:fee").await;
            }

            InvalidationEvent::PartnerConfigUpdated { partner_id } => {
                let key = ConfigKey::new(partner_id.to_string()).to_string();
                self.delete_key(&key, "v1:partner").await;
            }

            InvalidationEvent::PartnerLiquidityUpdated { partner_id } => {
                let key = LiquidityKey::new(partner_id.to_string()).to_string();
                self.delete_key(&key, "v1:partner").await;
            }
        }

        self.log_write_through(&event).await;
    }

    async fn delete_key(&self, key: &str, namespace: &str) {
        match CacheTrait::<serde_json::Value>::delete(&*self.redis, key).await {
            Ok(deleted) => {
                if deleted {
                    info!(key, namespace, "Write-through: cache key invalidated");
                } else {
                    debug!(key, namespace, "Write-through: key not in cache (miss)");
                }
            }
            Err(e) => {
                warn!(key, error = %e, "Write-through: cache delete failed (degraded)");
            }
        }
    }

    async fn delete_pattern(&self, pattern: &str, namespace: &str) {
        match CacheTrait::<serde_json::Value>::delete_pattern(&*self.redis, pattern).await {
            Ok(n) => info!(pattern, namespace, deleted = n, "Write-through: pattern invalidated"),
            Err(e) => warn!(pattern, error = %e, "Write-through: pattern delete failed"),
        }
    }

    async fn log_write_through(&self, event: &InvalidationEvent) {
        let Some(ref pool) = self.pool else { return };

        let (namespace, pattern) = match event {
            InvalidationEvent::WalletBalanceChanged { address } =>
                ("v1:wallet".to_string(), format!("v1:wallet:balance:{}", address)),
            InvalidationEvent::ExchangeRateUpdated { from, to } =>
                ("v1:rate".to_string(), format!("v1:rate:{}:{}", from, to)),
            InvalidationEvent::UserProfileUpdated { user_id } =>
                ("v1:user".to_string(), format!("v1:user:{}:profile", user_id)),
            InvalidationEvent::UserOnboardingUpdated { user_id } =>
                ("v1:user".to_string(), format!("v1:user:{}:onboarding", user_id)),
            InvalidationEvent::FeeStructureUpdated =>
                ("v1:fee".to_string(), "v1:fee:*".to_string()),
            InvalidationEvent::PartnerConfigUpdated { partner_id } =>
                ("v1:partner".to_string(), format!("v1:partner:{}:config", partner_id)),
            InvalidationEvent::PartnerLiquidityUpdated { partner_id } =>
                ("v1:partner".to_string(), format!("v1:partner:{}:liquidity", partner_id)),
        };

        let _ = sqlx::query(
            r#"INSERT INTO cache_invalidation_logs
               (target_namespace, pattern_used, reason)
               VALUES ($1, $2, 'write_through')"#,
        )
        .bind(&namespace)
        .bind(&pattern)
        .execute(pool.as_ref())
        .await;
    }
}
