# Database Scaling Implementation Guide

**Status**: Phase 1 Foundation Complete  
**Last Updated**: 2026-06-01  
**Target Completion**: 2026-07-15

---

## Summary

This document provides implementation details for the cNGN database scaling architecture covering read replica routing, logical sharding, and write operation isolation.

---

## Implemented Components

### ✅ 1. Read Replica Router (`src/database/read_replica_router.rs`)

**Purpose**: Route queries to replicas for eventual-consistency reads, fallback to primary.

**Key Features**:
- `ConsistencyLevel` enum: `Eventual`, `ReadYourWrites`, `Serializable`
- Per-replica health tracking with lag detection
- Automatic failover when replica lag > threshold
- Background health check loop (configurable interval)
- Exponential backoff on connection failures

**Usage Example**:
```rust
let router = ReadReplicaRouter::new(
    primary_pool,
    vec!["postgres://replica1:5432/aframp".to_string()],
    None,
    5000, // 5s lag threshold
).await?;

// Route eventual-consistency read to replica
let result = router.execute_read(
    |pool| sqlx::query("SELECT * FROM transactions WHERE id = $1").bind(id),
    ConsistencyLevel::Eventual,
    Some("shard_0"),
).await?;
```

**Configuration**:
```env
DB_REPLICA_LAG_THRESHOLD_MS=5000
DB_HEALTH_CHECK_INTERVAL_SECS=30
DB_REPLICA_FAILURE_THRESHOLD=5
```

---

### ✅ 2. Shard Manager (`src/database/shard_manager.rs`)

**Purpose**: Manage logical sharding with hot-reload from `shard_registry` table.

**Key Features**:
- FNV-1a consistent hashing with configurable slots (16384 default)
- Shard status lifecycle: `active` → `draining` → `offline`
- Per-shard primary + replica pools
- Hot-reload on shard config changes (background task)
- Supports corridor-based + time-based sharding

**Schema - Shard Registry**:
```sql
CREATE TABLE shard_registry (
    shard_id INT PRIMARY KEY,
    corridor_id TEXT NOT NULL,
    week_id INT,  -- Optional for time-based splits
    primary_dsn TEXT NOT NULL,
    replica_dsns TEXT[] DEFAULT ARRAY[]::TEXT[],
    status TEXT CHECK (status IN ('active', 'draining', 'offline')),
    max_connections INT DEFAULT 8,
    weight INT DEFAULT 1,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
```

**Usage Example**:
```rust
let coordinator_pool = init_pool("postgres://coordinator:5432/aframp").await?;
let shard_mgr = ShardManager::new(
    Arc::new(coordinator_pool),
    Duration::from_secs(60), // Refresh interval
).await?;

// Route a key to a shard
let shard_id = shard_mgr.route_key("merchant_ng_001").await?;

// Get pools for a shard
let write_pool = shard_mgr.get_write_pool(shard_id).await?;
let read_pool = shard_mgr.get_read_pool(shard_id).await?;
```

**Configuration**:
```env
DATABASE_COORDINATOR_URL=postgres://coordinator:5432/aframp
SHARD_REGISTRY_REFRESH_INTERVAL_SECS=60
```

---

### ✅ 3. Write Isolation Manager (`src/database/write_isolation.rs`)

**Purpose**: Isolate critical writes (settlement, compliance) from analytics writes.

**Key Features**:
- Separate connection pools per operation type
- Configurable isolation levels: SERIALIZABLE, REPEATABLE READ, READ COMMITTED
- Exponential backoff retry logic with circuit breaker
- Per-operation-type metrics (success, failure, latency)
- Automatic rollback on serialization conflicts

**Operation Types**:
```rust
pub enum WriteOperationType {
    Settlement,      // SERIALIZABLE, 3 retries
    AuditLedger,     // REPEATABLE READ, 1 retry
    Analytics,       // READ COMMITTED, 5 retries
    Compliance,      // SERIALIZABLE, 3 retries
    Verification,    // SERIALIZABLE, 3 retries
}
```

**Usage Example**:
```rust
let write_mgr = WriteIsolationManager::new(
    "postgres://settlement:5432/aframp",
    "postgres://audit:5432/aframp",
    "postgres://analytics:5432/aframp",
    "postgres://compliance:5432/aframp",
    "postgres://verification:5432/aframp",
).await?;

// Execute a settlement write with automatic isolation and retries
write_mgr
    .execute_write(WriteOperationType::Settlement, |tx| {
        Box::pin(async move {
            sqlx::query("INSERT INTO settlement_batches ...")
                .execute(&mut **tx)
                .await
                .map_err(|e| DatabaseError::from_sqlx(e))?;
            Ok(())
        })
    })
    .await?;
```

**Configuration**:
```env
DB_SETTLEMENT_POOL_MAX_CONNECTIONS=24
DB_AUDIT_POOL_MAX_CONNECTIONS=16
DB_ANALYTICS_POOL_MAX_CONNECTIONS=32
```

---

### ✅ 4. Ledger Query Accelerator (`src/database/ledger_cache.rs`)

**Purpose**: Pre-aggregate queries and cache results for ledger verification.

**Key Features**:
- In-memory result caching with TTL
- Materialized view management
- Pattern-based cache invalidation
- Query result filtering and eviction

**Materialized Views**:

1. **settlement_summaries_by_corridor_week**
   - Aggregates settlement batches by corridor and week
   - Pre-computed gross, fees, counts
   - Refreshed every 5 minutes

2. **transaction_stats_by_corridor_day**
   - Transaction statistics by corridor, day, type, status
   - Sums, averages, and counts
   - Refreshed every hour

**Usage Example**:
```rust
let cache = Arc::new(QueryResultCache::new(10000)); // Max 10k entries
let view_mgr = Arc::new(MaterializedViewManager::new(
    Arc::new(pool),
    Duration::from_secs(300), // 5 min refresh
));

let accelerator = LedgerQueryAccelerator::new(
    Arc::new(pool),
    cache.clone(),
    view_mgr,
);

// Query with automatic caching (TTL 1 hour)
let summary = accelerator
    .settlement_summary_by_corridor_week("NG", 202601)
    .await?;

// Invalidate cache for corridor
accelerator.invalidate_corridor_cache("NG").await;
```

---

### ✅ 5. Monitoring & Metrics (`src/database/monitoring.rs`)

**Purpose**: Track database scaling health via Prometheus metrics.

**Key Metrics**:

| Metric | Type | Labels | Purpose |
|--------|------|--------|---------|
| `db_read_latency_seconds` | Histogram | `shard_id`, `consistency_level` | Track read query latency by shard and consistency |
| `db_write_latency_seconds` | Histogram | `operation_type`, `shard_id` | Track write latency by operation type |
| `db_replica_lag_seconds` | Gauge | `replica_id`, `shard_id` | Monitor replication lag |
| `db_replica_health` | Gauge | `replica_id`, `shard_id` | Replica health status (1=healthy, 0=down) |
| `db_connection_pool_utilization` | Gauge | `pool_type`, `shard_id` | Track pool saturation (0-1) |
| `db_settlement_write_latency_seconds` | Histogram | — | Settlement write performance |
| `db_settlement_serialization_conflicts_total` | Counter | — | Track serialization failures |
| `db_circuit_breaker_state` | Gauge | `operation_type` | Circuit breaker status |
| `db_cache_hits_total` | Counter | — | Query cache effectiveness |

**Usage Example**:
```rust
use crate::database::monitoring::{init_metrics, MetricRecorder};

// Initialize metrics at startup
init_metrics()?;

// Record a read operation
let start = Instant::now();
let result = router.execute_read(...).await?;
MetricRecorder::record_read_operation("shard_0", "eventual", start.elapsed().as_secs_f64());

// Record cache hit
MetricRecorder::record_cache_hit();
```

---

## Deployment Steps

### Step 1: Database Schema Setup

```bash
# Run migrations in order
sqlx migrate run --database-url $DATABASE_URL

# This applies:
# - 20260601000000_database_scaling_shard_registry.sql
# - 20260601000100_database_scaling_acceleration_views.sql
```

### Step 2: Initialize Shard Registry

```sql
INSERT INTO shard_registry (
    shard_id, corridor_id, week_id,
    primary_dsn, replica_dsns,
    status, max_connections, weight
) VALUES
    (0, 'NG', 202601,
     'postgres://ng-w1-primary:5432/aframp',
     ARRAY['postgres://ng-w1-replica1:5432/aframp', 'postgres://ng-w1-replica2:5432/aframp'],
     'active', 16, 1),
    (1, 'GH', 202601,
     'postgres://gh-w1-primary:5432/aframp',
     ARRAY['postgres://gh-w1-replica1:5432/aframp'],
     'active', 16, 1),
    (2, 'KE', 202601,
     'postgres://ke-w1-primary:5432/aframp',
     ARRAY['postgres://ke-w1-replica1:5432/aframp'],
     'active', 16, 1);
```

### Step 3: Environment Configuration

```bash
# Read Replica Configuration
export DB_REPLICA_LAG_THRESHOLD_MS=5000
export DB_HEALTH_CHECK_INTERVAL_SECS=30

# Shard Manager Configuration
export DATABASE_COORDINATOR_URL=postgres://coordinator:5432/aframp
export SHARD_REGISTRY_REFRESH_INTERVAL_SECS=60

# Write Isolation Pools
export DB_SETTLEMENT_POOL_MAX_CONNECTIONS=24
export DB_AUDIT_POOL_MAX_CONNECTIONS=16
export DB_ANALYTICS_POOL_MAX_CONNECTIONS=32

# Query Acceleration
export DB_QUERY_CACHE_MAX_ENTRIES=10000
export DB_MATERIALIZED_VIEW_REFRESH_INTERVAL_SECS=300
```

### Step 4: Application Integration

```rust
// In main.rs or initialization code
use crate::database::{
    read_replica_router::ReadReplicaRouter,
    shard_manager::ShardManager,
    write_isolation::WriteIsolationManager,
    ledger_cache::LedgerQueryAccelerator,
    monitoring::init_metrics,
};

// Initialize metrics
init_metrics()?;

// Initialize coordinator pool
let coordinator_pool = init_pool(&config.database.url).await?;

// Initialize shard manager
let shard_mgr = Arc::new(
    ShardManager::new(
        Arc::new(coordinator_pool),
        Duration::from_secs(60),
    )
    .await?
);

// Initialize read replica router
let read_router = Arc::new(
    ReadReplicaRouter::new(
        Arc::new(primary_pool),
        vec![replica_dsn1, replica_dsn2],
        None,
        5000,
    )
    .await?
);

// Initialize write isolation manager
let write_mgr = Arc::new(
    WriteIsolationManager::new(
        &settlement_dsn,
        &audit_dsn,
        &analytics_dsn,
        &compliance_dsn,
        &verification_dsn,
    )
    .await?
);

// Initialize query accelerator
let cache = Arc::new(QueryResultCache::new(10000));
let view_mgr = Arc::new(MaterializedViewManager::new(
    Arc::new(pool),
    Duration::from_secs(300),
));
let accelerator = Arc::new(LedgerQueryAccelerator::new(
    Arc::new(pool),
    cache,
    view_mgr,
));
```

---

## Testing

### Unit Tests

```bash
# Test individual components
cargo test database::read_replica_router
cargo test database::shard_manager
cargo test database::write_isolation
cargo test database::ledger_cache
```

### Integration Tests

```bash
# Test end-to-end with real database
TEST_DATABASE_URL=postgres://test:test@localhost/aframp_test \
    cargo test --test database_scaling_integration

# Run load test
cargo run --example database_scaling_load_test -- \
    --threads 10 \
    --duration 300 \
    --target-throughput 300
```

### Performance Baselines

| Operation | Target | Alert If > |
|-----------|--------|-----------|
| Read (eventual) | <50ms p99 | 200ms |
| Read (consistent) | <100ms p99 | 300ms |
| Write (settlement) | <200ms p99 | 500ms |
| Replica lag | <1s p99 | 5s |
| Pool utilization | <60% | >80% |

---

## Runbooks

### Shard Addition (Zero-Downtime)

1. **Provision new database**
   ```bash
   # Provision replica infrastructure for new shard
   terraform apply -target=aws_rds_instance.shard_3_primary
   ```

2. **Insert into shard_registry** (with `status='draining'`)
   ```sql
   INSERT INTO shard_registry (..., status='draining')
   VALUES (3, 'SN', 202601, 'postgres://sn-w1-primary:5432/aframp', ...);
   ```

3. **Data migration** (if splitting existing shard)
   ```sql
   INSERT INTO shard_3_transactions
   SELECT * FROM shard_0_transactions
   WHERE corridor_id = 'SN' AND year_week = 202601;
   ```

4. **Update shard_registry to active**
   ```sql
   UPDATE shard_registry SET status='active' WHERE shard_id=3;
   ```

5. **Monitor metrics** for data consistency

### Replica Failover

Automatic failover happens when:
- Replica lag > 5s (configurable)
- 5+ consecutive connection failures (configurable)

Manual failover:
```sql
-- Promote replica to primary
-- This is database-specific; for PostgreSQL:
pg_ctl promote -D /path/to/replica

-- Update shard_registry
UPDATE shard_registry SET primary_dsn='new_primary_url' WHERE shard_id=0;
```

### Circuit Breaker Reset

If circuit breaker opens (10+ consecutive write failures):
```bash
# Restart write isolation manager or:
curl -X POST http://localhost:8080/admin/reset-circuit-breaker \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d '{"operation_type": "settlement"}'
```

---

## Monitoring Dashboard Queries

### Grafana Queries

**Read Latency by Shard**:
```prometheus
histogram_quantile(0.99, rate(db_read_latency_seconds_bucket[5m])) by (shard_id)
```

**Replica Lag Alert**:
```prometheus
db_replica_lag_seconds > 5 OR db_replica_health == 0
```

**Connection Pool Saturation**:
```prometheus
db_connection_pool_utilization > 0.8
```

**Settlement Write Success Rate**:
```prometheus
rate(db_write_operations_total{operation_type="settlement"}[5m]) - rate(db_settlement_commit_failures_total[5m])
```

---

## Next Steps

- [ ] Integration with transaction repository
- [ ] Load testing at 300+ TPS
- [ ] Replica lag monitoring dashboard
- [ ] Runbook automation (Kubernetes operators)
- [ ] Multi-region replication setup
- [ ] Query plan optimization
- [ ] Cross-shard transaction support (optional, complex)

---

## References

- [Database Scaling Architecture](./DATABASE_SCALING_ARCHITECTURE.md)
- [PostgreSQL Read Replicas](https://www.postgresql.org/docs/current/warm-standby.html)
- [Consistent Hashing](https://www.aHash-based Partitioning)
- [Circuit Breaker Pattern](https://martinfowler.com/bliki/CircuitBreaker.html)

