// ============================================================================
// Financial Reconciliation Worker
// ============================================================================
// Hourly reconciliation between internal ledger and on-chain Stellar state
// with automated circuit-breaker safety controls.
// ============================================================================

use crate::database::PgPool;
use crate::stellar::StellarClient;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};
use uuid::Uuid;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct ReconciliationConfig {
    /// Interval between reconciliation runs
    pub interval: Duration,
    /// Maximum acceptable drift in stroops before alerting
    pub max_drift_stroops: i64,
    /// Maximum acceptable drift percentage
    pub max_drift_percentage: Decimal,
    /// Enable automatic circuit breaker tripping
    pub auto_trip_enabled: bool,
    /// Maximum transactions to verify per run
    pub max_transactions_per_run: usize,
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(3600), // 1 hour
            max_drift_stroops: 50_000_000,       // 5 XLM
            max_drift_percentage: Decimal::new(50, 2), // 0.50%
            auto_trip_enabled: true,
            max_transactions_per_run: 50_000,
        }
    }
}

// ============================================================================
// Reconciliation Result Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationSnapshot {
    pub id: i64,
    pub snapshot_time: DateTime<Utc>,
    pub internal_balance_stroops: i64,
    pub internal_transaction_count: i64,
    pub stellar_balance_stroops: i64,
    pub stellar_sequence_number: i64,
    pub stellar_account_id: String,
    pub balance_drift_stroops: i64,
    pub drift_percentage: Decimal,
    pub is_reconciled: bool,
    pub reconciliation_duration_ms: i32,
    pub transactions_verified: i32,
}

#[derive(Debug)]
struct InternalLedgerState {
    balance_stroops: i64,
    transaction_count: i64,
    last_tx_id: Option<Uuid>,
}

#[derive(Debug)]
struct StellarAccountState {
    balance_stroops: i64,
    sequence_number: i64,
    account_id: String,
}

// ============================================================================
// Reconciliation Worker
// ============================================================================

pub struct ReconciliationWorker {
    pool: PgPool,
    stellar_client: Arc<StellarClient>,
    config: ReconciliationConfig,
}

impl ReconciliationWorker {
    pub fn new(
        pool: PgPool,
        stellar_client: Arc<StellarClient>,
        config: ReconciliationConfig,
    ) -> Self {
        Self {
            pool,
            stellar_client,
            config,
        }
    }

    /// Start the reconciliation worker loop
    pub async fn start(self: Arc<Self>) {
        info!(
            "Starting reconciliation worker with interval: {:?}",
            self.config.interval
        );

        let mut interval = time::interval(self.config.interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            if let Err(e) = self.run_reconciliation().await {
                error!("Reconciliation run failed: {:#}", e);
            }
        }
    }

    /// Execute a single reconciliation run
    pub async fn run_reconciliation(&self) -> Result<ReconciliationSnapshot> {
        let start_time = std::time::Instant::now();
        info!("Starting reconciliation run");

        // 1. Get internal ledger state
        let internal_state = self.get_internal_ledger_state().await?;
        
        // 2. Get on-chain Stellar state
        let stellar_state = self.get_stellar_account_state().await?;
        
        // 3. Calculate drift
        let balance_drift = internal_state.balance_stroops - stellar_state.balance_stroops;
        let drift_percentage = if stellar_state.balance_stroops > 0 {
            Decimal::from(balance_drift.abs())
                / Decimal::from(stellar_state.balance_stroops)
                * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // 4. Verify recent transactions (sample)
        let transactions_verified = self
            .verify_recent_transactions(self.config.max_transactions_per_run)
            .await?;

        // 5. Determine if reconciled
        let is_reconciled = balance_drift.abs() <= self.config.max_drift_stroops
            && drift_percentage <= self.config.max_drift_percentage;

        // 6. Record snapshot
        let duration_ms = start_time.elapsed().as_millis() as i32;
        
        let snapshot_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO reconciliation_ledger_snaps (
                internal_balance_stroops, internal_transaction_count, internal_last_tx_id,
                stellar_balance_stroops, stellar_sequence_number, stellar_account_id,
                balance_drift_stroops, drift_percentage, is_reconciled,
                reconciliation_duration_ms, transactions_verified
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id
            "#,
        )
        .bind(internal_state.balance_stroops)
        .bind(internal_state.transaction_count)
        .bind(internal_state.last_tx_id)
        .bind(stellar_state.balance_stroops)
        .bind(stellar_state.sequence_number)
        .bind(&stellar_state.account_id)
        .bind(balance_drift)
        .bind(drift_percentage)
        .bind(is_reconciled)
        .bind(duration_ms)
        .bind(transactions_verified as i32)
        .fetch_one(&self.pool)
        .await
        .context("Failed to insert reconciliation snapshot")?;

        // 7. Check circuit breaker thresholds
        if !is_reconciled && self.config.auto_trip_enabled {
            self.check_and_trip_circuit_breaker(balance_drift, snapshot_id)
                .await?;
        }

        info!(
            "Reconciliation complete: drift={} stroops ({:.4}%), reconciled={}, duration={}ms",
            balance_drift, drift_percentage, is_reconciled, duration_ms
        );

        Ok(ReconciliationSnapshot {
            id: snapshot_id,
            snapshot_time: Utc::now(),
            internal_balance_stroops: internal_state.balance_stroops,
            internal_transaction_count: internal_state.transaction_count,
            stellar_balance_stroops: stellar_state.balance_stroops,
            stellar_sequence_number: stellar_state.sequence_number,
            stellar_account_id: stellar_state.account_id,
            balance_drift_stroops: balance_drift,
            drift_percentage,
            is_reconciled,
            reconciliation_duration_ms: duration_ms,
            transactions_verified: transactions_verified as i32,
        })
    }

    /// Get the current internal ledger state
    async fn get_internal_ledger_state(&self) -> Result<InternalLedgerState> {
        let row = sqlx::query(
            r#"
            SELECT 
                COALESCE(SUM(
                    CASE 
                        WHEN type = 'CREDIT' THEN amount_stroops
                        WHEN type = 'DEBIT' THEN -amount_stroops
                        ELSE 0
                    END
                ), 0) AS balance_stroops,
                COUNT(*) AS transaction_count,
                MAX(id) AS last_tx_id
            FROM payment_ledger
            WHERE status = 'COMPLETED'
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to query internal ledger state")?;

        Ok(InternalLedgerState {
            balance_stroops: row.try_get("balance_stroops")?,
            transaction_count: row.try_get("transaction_count")?,
            last_tx_id: row.try_get("last_tx_id")?,
        })
    }

    /// Get the current on-chain Stellar account state
    async fn get_stellar_account_state(&self) -> Result<StellarAccountState> {
        // This would integrate with your actual Stellar client
        // Placeholder implementation:
        let account_id = "SYSTEM_ISSUER_ACCOUNT"; // From config
        
        // In production, this would call:
        // let account = self.stellar_client.get_account(account_id).await?;
        
        // Placeholder values
        Ok(StellarAccountState {
            balance_stroops: 1_000_000_000_000, // 100,000 XLM
            sequence_number: 123456789,
            account_id: account_id.to_string(),
        })
    }

    /// Verify a sample of recent transactions against on-chain data
    async fn verify_recent_transactions(&self, limit: usize) -> Result<usize> {
        let rows = sqlx::query(
            r#"
            SELECT id, stellar_tx_hash, amount_stroops
            FROM payment_ledger
            WHERE stellar_tx_hash IS NOT NULL
            AND status = 'COMPLETED'
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        let mut verified = 0;

        for row in rows {
            let tx_hash: String = row.try_get("stellar_tx_hash")?;
            
            // In production, verify against Stellar:
            // let exists = self.stellar_client.verify_transaction(&tx_hash).await?;
            let exists = true; // Placeholder
            
            if exists {
                verified += 1;
            }
        }

        Ok(verified)
    }

    /// Check circuit breaker thresholds and trip if necessary
    async fn check_and_trip_circuit_breaker(
        &self,
        drift_stroops: i64,
        snapshot_id: i64,
    ) -> Result<()> {
        let should_trip: bool = sqlx::query_scalar(
            "SELECT check_circuit_breaker_thresholds('GLOBAL_RECONCILIATION', $1, $2)",
        )
        .bind(drift_stroops)
        .bind(drift_stroops.abs()) // total balance approximation
        .fetch_one(&self.pool)
        .await?;

        if should_trip {
            warn!("Drift threshold exceeded, tripping circuit breaker");
            
            sqlx::query(
                "SELECT trip_circuit_breaker('GLOBAL_RECONCILIATION', $1, $2, $3)",
            )
            .bind(format!("Drift detected: {} stroops", drift_stroops))
            .bind(drift_stroops)
            .bind(snapshot_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Check if a circuit breaker is currently tripped
    pub async fn is_circuit_breaker_tripped(&self, circuit_name: &str) -> Result<bool> {
        let is_tripped: bool = sqlx::query_scalar(
            "SELECT is_circuit_breaker_tripped($1)",
        )
        .bind(circuit_name)
        .fetch_one(&self.pool)
        .await?;

        Ok(is_tripped)
    }

    /// Manually reset a circuit breaker (operator action)
    pub async fn reset_circuit_breaker(
        &self,
        circuit_name: &str,
        operator_id: Uuid,
        reason: &str,
    ) -> Result<()> {
        sqlx::query("SELECT reset_circuit_breaker($1, $2, $3)")
            .bind(circuit_name)
            .bind(operator_id)
            .bind(reason)
            .execute(&self.pool)
            .await?;

        info!("Circuit breaker reset: {}", circuit_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconciliation_config_default() {
        let config = ReconciliationConfig::default();
        assert_eq!(config.interval.as_secs(), 3600);
        assert_eq!(config.max_drift_stroops, 50_000_000);
    }
}
