//! Database Scaling Monitoring and Observability (Issue #XXX)
//!
//! Provides:
//! - Prometheus metrics for shard health
//! - Replica lag monitoring
//! - Write operation latency tracking
//! - Connection pool saturation metrics
//! - Query performance histograms

use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntGaugeVec,
    Registry, Opts,
};
use std::sync::OnceLock;

// ─────────────────────────────────────────────────────────────────────────────
// Global Metrics Registry
// ─────────────────────────────────────────────────────────────────────────────

static METRICS_REGISTRY: OnceLock<DatabaseMetrics> = OnceLock::new();

pub fn init_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let metrics = DatabaseMetrics::new()?;
    METRICS_REGISTRY
        .set(metrics)
        .map_err(|_| "Metrics already initialized".into())
}

pub fn get_metrics() -> Option<&'static DatabaseMetrics> {
    METRICS_REGISTRY.get()
}

// ─────────────────────────────────────────────────────────────────────────────
// Metrics Structs
// ─────────────────────────────────────────────────────────────────────────────

pub struct DatabaseMetrics {
    // Read/Write Operation Metrics
    pub read_latency_histogram: HistogramVec,
    pub write_latency_histogram: HistogramVec,
    pub read_operations_total: CounterVec,
    pub write_operations_total: CounterVec,

    // Replica Metrics
    pub replica_lag_gauge: GaugeVec,
    pub replica_health_gauge: IntGaugeVec,
    pub replica_failovers_total: CounterVec,

    // Shard Metrics
    pub shard_transaction_count: GaugeVec,
    pub shard_active_gauge: IntGaugeVec,
    pub shard_rebalance_duration: HistogramVec,

    // Connection Pool Metrics
    pub connection_pool_utilization: GaugeVec,
    pub connection_pool_size: IntGaugeVec,
    pub connection_acquisitions_total: CounterVec,
    pub connection_acquisition_failures_total: CounterVec,

    // Query Performance Metrics
    pub query_latency_histogram: HistogramVec,
    pub slow_queries_total: CounterVec,
    pub queries_by_type: CounterVec,

    // Settlement Write Metrics
    pub settlement_write_latency: Histogram,
    pub settlement_commit_failures: Counter,
    pub settlement_serialization_conflicts: Counter,

    // Audit Ledger Metrics
    pub audit_ledger_writes_total: Counter,
    pub audit_ledger_append_failures: Counter,

    // Circuit Breaker Metrics
    pub circuit_breaker_state: IntGaugeVec,
    pub circuit_breaker_trips_total: CounterVec,

    // Cache Metrics
    pub cache_hits_total: Counter,
    pub cache_misses_total: Counter,
    pub cache_evictions_total: Counter,
}

impl DatabaseMetrics {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let registry = &Registry::new();

        // Read/Write Operation Metrics
        let read_latency_histogram = HistogramVec::new(
            HistogramOpts::new(
                "db_read_latency_seconds",
                "Read operation latency in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0,
            ]),
            &["shard_id", "consistency_level"],
        )?;
        registry.register(Box::new(read_latency_histogram.clone()))?;

        let write_latency_histogram = HistogramVec::new(
            HistogramOpts::new(
                "db_write_latency_seconds",
                "Write operation latency in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0,
            ]),
            &["operation_type", "shard_id"],
        )?;
        registry.register(Box::new(write_latency_histogram.clone()))?;

        let read_operations_total = CounterVec::new(
            Opts::new("db_read_operations_total", "Total read operations"),
            &["shard_id", "consistency_level"],
        )?;
        registry.register(Box::new(read_operations_total.clone()))?;

        let write_operations_total = CounterVec::new(
            Opts::new("db_write_operations_total", "Total write operations"),
            &["operation_type", "shard_id"],
        )?;
        registry.register(Box::new(write_operations_total.clone()))?;

        // Replica Metrics
        let replica_lag_gauge = GaugeVec::new(
            Opts::new("db_replica_lag_seconds", "Replica lag in seconds"),
            &["replica_id", "shard_id"],
        )?;
        registry.register(Box::new(replica_lag_gauge.clone()))?;

        let replica_health_gauge = IntGaugeVec::new(
            Opts::new("db_replica_health", "Replica health (1=healthy, 0=unhealthy)"),
            &["replica_id", "shard_id"],
        )?;
        registry.register(Box::new(replica_health_gauge.clone()))?;

        let replica_failovers_total = CounterVec::new(
            Opts::new("db_replica_failovers_total", "Total replica failovers"),
            &["replica_id", "shard_id"],
        )?;
        registry.register(Box::new(replica_failovers_total.clone()))?;

        // Shard Metrics
        let shard_transaction_count = GaugeVec::new(
            Opts::new("db_shard_transactions_total", "Total transactions per shard"),
            &["shard_id", "corridor_id"],
        )?;
        registry.register(Box::new(shard_transaction_count.clone()))?;

        let shard_active_gauge = IntGaugeVec::new(
            Opts::new("db_shard_active", "Shard active status (1=active, 0=inactive)"),
            &["shard_id", "corridor_id"],
        )?;
        registry.register(Box::new(shard_active_gauge.clone()))?;

        let shard_rebalance_duration = HistogramVec::new(
            HistogramOpts::new(
                "db_shard_rebalance_duration_seconds",
                "Shard rebalancing duration",
            )
            .buckets(vec![1.0, 5.0, 10.0, 30.0, 60.0, 300.0]),
            &["shard_id"],
        )?;
        registry.register(Box::new(shard_rebalance_duration.clone()))?;

        // Connection Pool Metrics
        let connection_pool_utilization = GaugeVec::new(
            Opts::new("db_connection_pool_utilization", "Connection pool utilization (0-1)"),
            &["pool_type", "shard_id"],
        )?;
        registry.register(Box::new(connection_pool_utilization.clone()))?;

        let connection_pool_size = IntGaugeVec::new(
            Opts::new("db_connection_pool_size", "Connection pool size"),
            &["pool_type", "shard_id"],
        )?;
        registry.register(Box::new(connection_pool_size.clone()))?;

        let connection_acquisitions_total = CounterVec::new(
            Opts::new("db_connection_acquisitions_total", "Total connection acquisitions"),
            &["pool_type", "shard_id"],
        )?;
        registry.register(Box::new(connection_acquisitions_total.clone()))?;

        let connection_acquisition_failures_total = CounterVec::new(
            Opts::new(
                "db_connection_acquisition_failures_total",
                "Total connection acquisition failures",
            ),
            &["pool_type", "shard_id"],
        )?;
        registry.register(Box::new(
            connection_acquisition_failures_total.clone(),
        ))?;

        // Query Performance Metrics
        let query_latency_histogram = HistogramVec::new(
            HistogramOpts::new("db_query_latency_seconds", "Query latency by type"),
            &["query_type"],
        )?;
        registry.register(Box::new(query_latency_histogram.clone()))?;

        let slow_queries_total = CounterVec::new(
            Opts::new("db_slow_queries_total", "Queries exceeding threshold"),
            &["query_type"],
        )?;
        registry.register(Box::new(slow_queries_total.clone()))?;

        let queries_by_type = CounterVec::new(
            Opts::new("db_queries_by_type_total", "Total queries by type"),
            &["query_type"],
        )?;
        registry.register(Box::new(queries_by_type.clone()))?;

        // Settlement Write Metrics
        let settlement_write_latency = Histogram::with_opts(
            HistogramOpts::new(
                "db_settlement_write_latency_seconds",
                "Settlement write operation latency",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        )?;
        registry.register(Box::new(settlement_write_latency.clone()))?;

        let settlement_commit_failures =
            Counter::new("db_settlement_commit_failures_total", "Settlement commit failures")?;
        registry.register(Box::new(settlement_commit_failures.clone()))?;

        let settlement_serialization_conflicts = Counter::new(
            "db_settlement_serialization_conflicts_total",
            "Settlement serialization conflicts",
        )?;
        registry.register(Box::new(settlement_serialization_conflicts.clone()))?;

        // Audit Ledger Metrics
        let audit_ledger_writes_total =
            Counter::new("db_audit_ledger_writes_total", "Total audit ledger writes")?;
        registry.register(Box::new(audit_ledger_writes_total.clone()))?;

        let audit_ledger_append_failures =
            Counter::new("db_audit_ledger_append_failures_total", "Audit ledger append failures")?;
        registry.register(Box::new(audit_ledger_append_failures.clone()))?;

        // Circuit Breaker Metrics
        let circuit_breaker_state = IntGaugeVec::new(
            Opts::new("db_circuit_breaker_state", "Circuit breaker state (1=open, 0=closed)"),
            &["operation_type"],
        )?;
        registry.register(Box::new(circuit_breaker_state.clone()))?;

        let circuit_breaker_trips_total = CounterVec::new(
            Opts::new("db_circuit_breaker_trips_total", "Total circuit breaker trips"),
            &["operation_type"],
        )?;
        registry.register(Box::new(circuit_breaker_trips_total.clone()))?;

        // Cache Metrics
        let cache_hits_total =
            Counter::new("db_cache_hits_total", "Total cache hits")?;
        registry.register(Box::new(cache_hits_total.clone()))?;

        let cache_misses_total =
            Counter::new("db_cache_misses_total", "Total cache misses")?;
        registry.register(Box::new(cache_misses_total.clone()))?;

        let cache_evictions_total =
            Counter::new("db_cache_evictions_total", "Total cache evictions")?;
        registry.register(Box::new(cache_evictions_total.clone()))?;

        Ok(Self {
            read_latency_histogram,
            write_latency_histogram,
            read_operations_total,
            write_operations_total,
            replica_lag_gauge,
            replica_health_gauge,
            replica_failovers_total,
            shard_transaction_count,
            shard_active_gauge,
            shard_rebalance_duration,
            connection_pool_utilization,
            connection_pool_size,
            connection_acquisitions_total,
            connection_acquisition_failures_total,
            query_latency_histogram,
            slow_queries_total,
            queries_by_type,
            settlement_write_latency,
            settlement_commit_failures,
            settlement_serialization_conflicts,
            audit_ledger_writes_total,
            audit_ledger_append_failures,
            circuit_breaker_state,
            circuit_breaker_trips_total,
            cache_hits_total,
            cache_misses_total,
            cache_evictions_total,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Metric Recording Helpers
// ─────────────────────────────────────────────────────────────────────────────

pub struct MetricRecorder;

impl MetricRecorder {
    pub fn record_read_operation(
        shard_id: &str,
        consistency_level: &str,
        latency_seconds: f64,
    ) {
        if let Some(metrics) = get_metrics() {
            metrics
                .read_latency_histogram
                .with_label_values(&[shard_id, consistency_level])
                .observe(latency_seconds);
            metrics
                .read_operations_total
                .with_label_values(&[shard_id, consistency_level])
                .inc();
        }
    }

    pub fn record_write_operation(
        operation_type: &str,
        shard_id: &str,
        latency_seconds: f64,
    ) {
        if let Some(metrics) = get_metrics() {
            metrics
                .write_latency_histogram
                .with_label_values(&[operation_type, shard_id])
                .observe(latency_seconds);
            metrics
                .write_operations_total
                .with_label_values(&[operation_type, shard_id])
                .inc();
        }
    }

    pub fn record_replica_lag(replica_id: &str, shard_id: &str, lag_seconds: f64) {
        if let Some(metrics) = get_metrics() {
            metrics
                .replica_lag_gauge
                .with_label_values(&[replica_id, shard_id])
                .set(lag_seconds);
        }
    }

    pub fn record_cache_hit() {
        if let Some(metrics) = get_metrics() {
            metrics.cache_hits_total.inc();
        }
    }

    pub fn record_cache_miss() {
        if let Some(metrics) = get_metrics() {
            metrics.cache_misses_total.inc();
        }
    }
}
