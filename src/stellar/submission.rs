/// High-throughput Stellar transaction submission engine
///
/// Orchestrates channel pooling, sequence coordination, fee management,
/// and retry logic for parallelized, resilient transaction submissions.
use crate::stellar::channel_pool::ChannelPool;
use crate::stellar::error::{HorizonErrorCode, SubmissionError, SubmissionResult};
use crate::stellar::fee_engine::DynamicFeeEngine;
use crate::stellar::horizon::HorizonClient;
use crate::stellar::metrics::{MetricsTimer, StellarMetrics};
use crate::stellar::models::{
    BatchEnvelopeRequest, BatchSubmissionResult, FeeConfiguration, RetryPolicy, SubmissionMetrics,
    SubmissionQueueItem, TransactionLogEntry,
};
use crate::stellar::retry_state_machine::RetryStateMachine;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

/// Main submission engine coordinating all components
pub struct StellarSubmissionEngine {
    pool: PgPool,
    issuer_id: Uuid,
    channel_pool: Arc<ChannelPool>,
    fee_engine: Arc<DynamicFeeEngine>,
    horizon_client: Arc<HorizonClient>,
    retry_policy: RetryPolicy,
    metrics: Arc<StellarMetrics>,
    stale_threshold: Duration,
    confirmation_check_interval: std::time::Duration,
    batch_max_envelopes: usize,
}

impl StellarSubmissionEngine {
    /// Create a new submission engine.
    ///
    /// `rpc_endpoints` is an optional list of additional Stellar RPC / Horizon
    /// nodes used for round-robin load balancing.  When the list is non-empty
    /// every request to Horizon is distributed across the entire set (primary
    /// `horizon_url` + all entries in `rpc_endpoints`), satisfying the
    /// "Validator Interaction Tuning" requirement of Issue #401.
    pub async fn new(
        pool: PgPool,
        issuer_id: Uuid,
        horizon_url: String,
        rpc_endpoints: Vec<String>,
        fee_config: FeeConfiguration,
        retry_policy: RetryPolicy,
        metrics: Arc<StellarMetrics>,
    ) -> SubmissionResult<Self> {
        let channel_pool = Arc::new(
            ChannelPool::new(
                pool.clone(),
                issuer_id,
                3,    // circuit breaker threshold
                1000, // max in-flight per channel
            )
            .await?,
        );

        let fee_engine = Arc::new(DynamicFeeEngine::new(fee_config, horizon_url.clone()));

        // Build the Horizon client.  If additional RPC endpoints are provided,
        // combine them with the primary URL so the client can round-robin across
        // a load-balanced cluster of nodes.
        let raw_client = HorizonClient::new(horizon_url.clone());
        let horizon_client = if rpc_endpoints.is_empty() {
            raw_client
        } else {
            let mut all_endpoints = vec![horizon_url];
            all_endpoints.extend(rpc_endpoints);
            raw_client.with_rpc_endpoints(all_endpoints)
        };
        let horizon_client = Arc::new(horizon_client);

        Ok(Self {
            pool,
            issuer_id,
            channel_pool,
            fee_engine,
            horizon_client,
            retry_policy,
            metrics,
            stale_threshold: Duration::seconds(60), // 4 ledgers
            confirmation_check_interval: std::time::Duration::from_secs(5),
            batch_max_envelopes: 100,
        })
    }

    /// Submit a transaction envelope (XDR)
    pub async fn submit_transaction(
        &self,
        tx_envelope_xdr: &str,
        operation_count: i32,
    ) -> SubmissionResult<TransactionLogEntry> {
        let _timer = MetricsTimer::new(self.metrics.submission_duration_seconds.clone());

        if operation_count <= 0 || operation_count > 100 {
            return Err(SubmissionError::InvalidEnvelope(
                "operation_count must be between 1 and 100".to_string(),
            ));
        }

        // Calculate dynamic fee
        let fee = self.fee_engine.calculate_fee(operation_count).await?;
        let surge_percent = self.fee_engine.get_surge_percent().await?;
        let per_op_fee = fee / (operation_count.max(1) as i64);
        self.metrics
            .current_surge_fee_stroops
            .set(per_op_fee as f64);

        // Reserve sequence and select channel
        let (channel, sequence) = self.channel_pool.reserve_sequence().await?;

        // Create transaction envelope hash (XDR-based)
        let tx_hash = self.compute_tx_hash(tx_envelope_xdr)?;

        // Log transaction in database
        let log_entry = sqlx::query_as::<_, TransactionLogEntry>(
            r#"
            INSERT INTO stellar_transaction_logs (
                issuer_id, channel_id, submission_index, tx_envelope_hash,
                tx_envelope_xdr, submission_fee_stroops, surge_fee_percent,
                submitted_at, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
            RETURNING
                id, issuer_id, channel_id, submission_index, tx_envelope_hash,
                tx_envelope_xdr, submission_fee_stroops, surge_fee_percent,
                submission_attempt, submitted_at, confirmed_at, stellar_ledger_hash,
                stellar_ledger_number, stellar_tx_hash, last_error_code, last_error_reason,
                retry_count, next_retry_at, final_status, failure_reason, created_at
            "#,
        )
        .bind(self.issuer_id)
        .bind(channel.db_id)
        .bind(sequence)
        .bind(&tx_hash)
        .bind(tx_envelope_xdr)
        .bind(fee)
        .bind(surge_percent)
        .fetch_one(&self.pool)
        .await?;

        // Submit to Horizon
        match self
            .horizon_client
            .submit_transaction(tx_envelope_xdr)
            .await
        {
            Ok(horizon_tx) => {
                self.channel_pool
                    .mark_channel_success(channel.db_id)
                    .await?;
                self.metrics.tx_submitted_total.inc();

                // Update with stellar hash if immediately available
                if let Some(stellar_hash) = horizon_tx.hash.split('/').last() {
                    let _ = sqlx::query(
                        "UPDATE stellar_transaction_logs SET stellar_tx_hash = $1 WHERE id = $2",
                    )
                    .bind(stellar_hash)
                    .bind(log_entry.id)
                    .execute(&self.pool)
                    .await;
                }

                Ok(log_entry)
            }
            Err(e) => {
                self.metrics.tx_failed_total.inc();
                self.channel_pool
                    .mark_channel_failure(channel.db_id)
                    .await?;

                // Classify and record error
                let error_code = self.classify_error(&e);
                if let Some(code) = &error_code {
                    match code {
                        HorizonErrorCode::TxBadSeq => self.metrics.sequence_errors_total.inc(),
                        HorizonErrorCode::TxInsufficientFee => self.metrics.fee_errors_total.inc(),
                        _ if code.is_retryable() => self.metrics.transient_errors_total.inc(),
                        _ => {}
                    }
                }

                // Update log entry with error
                let error_code_str = self.error_code_for_forensics(&e);
                let _ = sqlx::query(
                    r#"
                    UPDATE stellar_transaction_logs
                    SET last_error_code = $1, last_error_reason = $2, last_error_at = NOW()
                    WHERE id = $3
                    "#,
                )
                .bind(error_code_str)
                .bind(e.to_string())
                .bind(log_entry.id)
                .execute(&self.pool)
                .await;

                let _ = self
                    .record_forensic_failure(
                        None,
                        Some(log_entry.id),
                        Some(channel.db_id),
                        &error_code_str,
                        &e.to_string(),
                        false,
                    )
                    .await;

                Err(e)
            }
        }
    }

    /// Poll for transaction confirmation
    pub async fn poll_confirmation(&self, tx_log_id: Uuid) -> SubmissionResult<bool> {
        let log_entry: TransactionLogEntry =
            sqlx::query_as("SELECT * FROM stellar_transaction_logs WHERE id = $1")
                .bind(tx_log_id)
                .fetch_one(&self.pool)
                .await?;

        if log_entry.confirmed_at.is_some() {
            return Ok(true);
        }

        if let Some(stellar_hash) = &log_entry.stellar_tx_hash {
            // Check if transaction is on-chain
            if let Some(horizon_tx) = self
                .horizon_client
                .poll_transaction_confirmation(stellar_hash, 10)
                .await?
            {
                let confirmation_delay = Utc::now()
                    .signed_duration_since(log_entry.submitted_at)
                    .num_seconds();

                // Check for confirmation delay alert (> 3 ledgers / 15s)
                if confirmation_delay > 15 {
                    let ledgers_to_confirm = (confirmation_delay / 5) as i32;
                    let _ = sqlx::query(
                        r#"
                        INSERT INTO stellar_confirmation_delay_alerts
                        (tx_log_id, submitted_at, ledgers_to_confirm, confirmation_time_seconds, alert_sent_at, created_at)
                        VALUES ($1, $2, $3, $4, NOW(), NOW())
                        "#,
                    )
                    .bind(tx_log_id)
                    .bind(log_entry.submitted_at)
                    .bind(ledgers_to_confirm)
                    .bind(confirmation_delay as f64)
                    .execute(&self.pool)
                    .await;
                }

                // Update transaction log
                sqlx::query(
                    r#"
                    UPDATE stellar_transaction_logs
                    SET confirmed_at = NOW(), stellar_tx_hash = $1, stellar_ledger_number = $2, final_status = 'confirmed'
                    WHERE id = $3
                    "#,
                )
                .bind(&horizon_tx.hash)
                .bind(horizon_tx.ledger)
                .bind(tx_log_id)
                .execute(&self.pool)
                .await?;

                self.channel_pool
                    .mark_channel_confirmed_sequence(
                        log_entry.channel_id,
                        log_entry.submission_index,
                    )
                    .await?;

                self.metrics.tx_confirmed_total.inc();
                self.metrics
                    .confirmation_delay_seconds
                    .observe(confirmation_delay as f64);

                return Ok(true);
            }
        }

        // Check for stale transactions
        let age = Utc::now().signed_duration_since(log_entry.submitted_at);
        if age > self.stale_threshold {
            sqlx::query(
                r#"
                UPDATE stellar_transaction_logs
                SET final_status = 'stale', failure_reason = 'confirmation timeout'
                WHERE id = $1
                "#,
            )
            .bind(tx_log_id)
            .execute(&self.pool)
            .await?;

            return Err(SubmissionError::LedgerCloseTimeout { attempts: 10 });
        }

        Ok(false)
    }

    /// Queue a single envelope for asynchronous processing.
    pub async fn enqueue_submission(
        &self,
        tx_envelope_xdr: &str,
        operation_count: i32,
    ) -> SubmissionResult<Uuid> {
        let tx_hash = self.compute_tx_hash(tx_envelope_xdr)?;
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO stellar_submission_queue
                (issuer_id, tx_envelope_hash, tx_envelope_xdr, operation_count, queue_status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, 'PENDING', NOW(), NOW())
            ON CONFLICT (issuer_id, tx_envelope_hash)
            DO UPDATE SET updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(self.issuer_id)
        .bind(tx_hash)
        .bind(tx_envelope_xdr)
        .bind(operation_count.max(1))
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    /// Batch enqueue up to 100 envelopes for throughput optimization.
    pub async fn enqueue_batch(
        &self,
        envelopes: Vec<BatchEnvelopeRequest>,
    ) -> SubmissionResult<BatchSubmissionResult> {
        if envelopes.is_empty() {
            return Ok(BatchSubmissionResult {
                accepted: 0,
                rejected: 0,
                queued_ids: vec![],
            });
        }

        let mut accepted = 0usize;
        let mut rejected = 0usize;
        let mut queued_ids = Vec::new();

        for env in envelopes.into_iter().take(self.batch_max_envelopes) {
            if env.operation_count <= 0 {
                rejected += 1;
                continue;
            }
            match self
                .enqueue_submission(&env.tx_envelope_xdr, env.operation_count)
                .await
            {
                Ok(id) => {
                    accepted += 1;
                    queued_ids.push(id);
                }
                Err(_) => rejected += 1,
            }
        }

        Ok(BatchSubmissionResult {
            accepted,
            rejected,
            queued_ids,
        })
    }

    /// Process one queue tick (PENDING/RETRYING -> SUBMITTED/CONFIRMED/FAILED).
    pub async fn process_submission_queue_tick(&self, limit: i64) -> SubmissionResult<usize> {
        let items: Vec<SubmissionQueueItem> = sqlx::query_as(
            r#"
            SELECT
                id, issuer_id, channel_id, tx_envelope_hash, tx_envelope_xdr, operation_count,
                queue_status, submission_attempt, last_error_code, last_error_reason,
                next_attempt_at, submitted_at, confirmed_at, created_at, updated_at
            FROM stellar_submission_queue
            WHERE issuer_id = $1
              AND queue_status IN ('PENDING', 'RETRYING', 'SUBMITTED')
              AND (next_attempt_at IS NULL OR next_attempt_at <= NOW())
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(self.issuer_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;

        let mut processed = 0usize;

        for item in items {
            processed += 1;
            if item.queue_status == "SUBMITTED" {
                if let Some(log_id) = self
                    .lookup_log_id_by_envelope(&item.tx_envelope_hash)
                    .await?
                {
                    if self.poll_confirmation(log_id).await.unwrap_or(false) {
                        let _ = sqlx::query(
                            "UPDATE stellar_submission_queue SET queue_status='CONFIRMED', confirmed_at=NOW(), updated_at=NOW() WHERE id = $1",
                        )
                        .bind(item.id)
                        .execute(&self.pool)
                        .await;
                    }
                }
                continue;
            }

            let mut retry_sm = RetryStateMachine::new(self.retry_policy.clone());
            if item.submission_attempt > 0 {
                for _ in 0..item.submission_attempt {
                    let _ = retry_sm.record_attempt(&SubmissionError::TransientNetworkError {
                        source: "replayed queue attempt".to_string(),
                        attempt: item.submission_attempt as u32,
                    });
                }
            }

            match self
                .submit_transaction(&item.tx_envelope_xdr, item.operation_count)
                .await
            {
                Ok(log) => {
                    let _ = sqlx::query(
                        r#"
                        UPDATE stellar_submission_queue
                        SET queue_status = 'SUBMITTED',
                            channel_id = $2,
                            submission_attempt = submission_attempt + 1,
                            submitted_at = NOW(),
                            updated_at = NOW()
                        WHERE id = $1
                        "#,
                    )
                    .bind(item.id)
                    .bind(log.channel_id)
                    .execute(&self.pool)
                    .await;
                }
                Err(err) => {
                    if retry_sm.should_rotate_channel(&err) {
                        let _ = self.channel_pool.rotate_channel().await;
                        self.metrics.channel_rotations_total.inc();
                    }

                    let retryable = retry_sm.should_retry(&err);
                    if retryable {
                        let _ = retry_sm.record_attempt(&err);
                        let next_retry = Utc::now()
                            + chrono::Duration::from_std(retry_sm.calculate_next_retry_delay())
                                .unwrap_or_else(|_| chrono::Duration::seconds(1));
                        let forensic_code = self.error_code_for_forensics(&err);
                        let forensic_reason = err.to_string();
                        let _ = sqlx::query(
                            r#"
                            UPDATE stellar_submission_queue
                            SET queue_status = 'RETRYING',
                                submission_attempt = submission_attempt + 1,
                                last_error_code = $2,
                                last_error_reason = $3,
                                next_attempt_at = $4,
                                updated_at = NOW()
                            WHERE id = $1
                            "#,
                        )
                        .bind(item.id)
                        .bind(&forensic_code)
                        .bind(&forensic_reason)
                        .bind(next_retry)
                        .execute(&self.pool)
                        .await;

                        let _ = self
                            .record_forensic_failure(
                                Some(item.id),
                                None,
                                item.channel_id,
                                &forensic_code,
                                &forensic_reason,
                                true,
                            )
                            .await;
                    } else {
                        let forensic_code = self.error_code_for_forensics(&err);
                        let forensic_reason = err.to_string();
                        let _ = sqlx::query(
                            r#"
                            UPDATE stellar_submission_queue
                            SET queue_status = 'FAILED',
                                submission_attempt = submission_attempt + 1,
                                last_error_code = $2,
                                last_error_reason = $3,
                                updated_at = NOW()
                            WHERE id = $1
                            "#,
                        )
                        .bind(item.id)
                        .bind(&forensic_code)
                        .bind(&forensic_reason)
                        .execute(&self.pool)
                        .await;

                        let _ = self
                            .record_forensic_failure(
                                Some(item.id),
                                None,
                                item.channel_id,
                                &forensic_code,
                                &forensic_reason,
                                false,
                            )
                            .await;
                    }
                }
            }
        }

        Ok(processed)
    }

    /// Start asynchronous submission/monitoring loop.
    pub fn start_background_queue_worker(
        self: Arc<Self>,
        batch_limit: i64,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                let _ = self.process_submission_queue_tick(batch_limit).await;
            }
        })
    }

    async fn lookup_log_id_by_envelope(
        &self,
        tx_envelope_hash: &str,
    ) -> SubmissionResult<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM stellar_transaction_logs WHERE issuer_id = $1 AND tx_envelope_hash = $2 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(self.issuer_id)
        .bind(tx_envelope_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.0))
    }

    async fn record_forensic_failure(
        &self,
        queue_id: Option<Uuid>,
        tx_log_id: Option<Uuid>,
        channel_id: Option<Uuid>,
        error_code: &str,
        error_reason: &str,
        retryable: bool,
    ) -> SubmissionResult<()> {
        sqlx::query(
            r#"
            INSERT INTO stellar_tx_forensic_failures
                (queue_id, tx_log_id, issuer_id, channel_id, error_code, error_reason, retryable, occurred_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
            "#,
        )
        .bind(queue_id)
        .bind(tx_log_id)
        .bind(self.issuer_id)
        .bind(channel_id)
        .bind(error_code)
        .bind(error_reason)
        .bind(retryable)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get channel pool statistics
    pub async fn get_pool_stats(&self) -> SubmissionResult<Vec<serde_json::Value>> {
        let stats = self.channel_pool.get_channel_stats().await?;

        let json_stats: Vec<_> = stats
            .iter()
            .map(|s| {
                serde_json::json!({
                    "channel_id": s.channel_id.to_string(),
                    "index": s.index,
                    "account_id": s.account_id,
                    "current_sequence": s.current_sequence,
                    "reserved_sequence": s.reserved_sequence,
                    "in_flight": s.in_flight,
                    "total_submitted": s.total_submitted,
                    "total_successful": s.total_successful,
                    "total_failed": s.total_failed,
                    "consecutive_failures": s.consecutive_failures,
                    "is_circuit_broken": s.is_circuit_broken,
                })
            })
            .collect();

        Ok(json_stats)
    }

    /// Compute transaction hash from XDR envelope
    fn compute_tx_hash(&self, tx_xdr: &str) -> SubmissionResult<String> {
        use sha2::{Digest, Sha256};

        let decoded = base64::decode(tx_xdr)
            .map_err(|e| SubmissionError::InvalidEnvelope(format!("XDR decode failed: {}", e)))?;

        let mut hasher = Sha256::new();
        hasher.update(&decoded);
        let hash = hasher.finalize();

        Ok(format!("{:x}", hash))
    }

    fn error_code_for_forensics(&self, error: &SubmissionError) -> String {
        match error {
            SubmissionError::BadSequence(_) => "txBAD_SEQ".to_string(),
            SubmissionError::InsufficientFee { .. } => "txINSUFFICIENT_FEE".to_string(),
            SubmissionError::MalformedTransaction(_) => "txMALFORMED".to_string(),
            SubmissionError::LedgerCloseTimeout { .. } => "txTOO_LATE".to_string(),
            SubmissionError::TransientNetworkError { .. } => "TRANSIENT_NETWORK".to_string(),
            SubmissionError::ChannelExhausted(_) => "CHANNEL_EXHAUSTED".to_string(),
            SubmissionError::HorizonApi(msg) => format!("HORIZON:{}", msg),
            other => format!("{:?}", other),
        }
    }

    /// Classify Horizon error for metrics
    fn classify_error(&self, error: &SubmissionError) -> Option<HorizonErrorCode> {
        match error {
            SubmissionError::BadSequence(_) => Some(HorizonErrorCode::TxBadSeq),
            SubmissionError::InsufficientFee { .. } => Some(HorizonErrorCode::TxInsufficientFee),
            SubmissionError::MalformedTransaction(_) => Some(HorizonErrorCode::TxMalformed),
            SubmissionError::TransientNetworkError { .. } => Some(HorizonErrorCode::Transient),
            SubmissionError::HorizonApi(msg) => Some(HorizonErrorCode::from_str(msg)),
            _ => None,
        }
    }

    /// Get current metrics snapshot
    pub async fn get_metrics_snapshot(&self) -> SubmissionResult<SubmissionMetrics> {
        let pool_capacity = self.channel_pool.get_pool_capacity_percent().await?;
        let stats = self.channel_pool.get_channel_stats().await?;

        let pending_confirmations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM stellar_submission_queue WHERE issuer_id = $1 AND queue_status IN ('PENDING','SUBMITTED','RETRYING')",
        )
        .bind(self.issuer_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let avg_finality_seconds: f64 = sqlx::query_scalar(
            "SELECT COALESCE(AVG(EXTRACT(EPOCH FROM (confirmed_at - submitted_at))), 0)::double precision FROM stellar_submission_queue WHERE issuer_id = $1 AND queue_status = 'CONFIRMED' AND confirmed_at IS NOT NULL AND submitted_at IS NOT NULL AND confirmed_at >= NOW() - INTERVAL '24 hours'",
        )
        .bind(self.issuer_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0.0);

        self.metrics.queue_depth.set(pending_confirmations as f64);
        self.metrics
            .avg_time_to_finality_seconds
            .set(avg_finality_seconds);

        Ok(SubmissionMetrics {
            timestamp: Utc::now(),
            throughput_tps: self.metrics.tx_throughput_tps.get(),
            avg_submission_duration_ms: avg_finality_seconds * 1000.0,
            current_surge_fee_stroops: self.metrics.current_surge_fee_stroops.get() as i64,
            channel_exhaustion_percent: pool_capacity,
            total_channels_active: stats.len() as u32,
            total_channels_inactive: 0,
            pending_confirmations: pending_confirmations.max(0) as u32,
            failed_submissions_24h: self.metrics.tx_failed_total.get_value() as u64,
        })
    }
}

// Helper imports
use base64;
use sha2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_tx_hash() {
        // This test would require a real transaction XDR
        // Tested in integration tests
    }
}
