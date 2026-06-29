# cNGN Database Scaling: Quick Start

**Created**: 2026-06-01  
**Status**: Ready for Phase 2 Integration  
**Related Issues**: #XXX (Database Scaling), #423 (HA Pool), #347 (Distributed Tracing)

---

## Overview

This directory contains the complete implementation of a **database scaling architecture** for cNGN, designed to handle write-heavy transaction volumes across multiple African corridors.

### What's Included

✅ **Read Replica Router** — Route queries to replicas with automatic failover  
✅ **Logical Sharding Framework** — Corridor-based sharding with hot-reload  
✅ **Write Operation Isolation** — Dedicated pools for settlement vs. analytics  
✅ **Ledger Query Acceleration** — Caching + materialized views  
✅ **Comprehensive Monitoring** — Prometheus metrics for all operations  
✅ **Migration Scripts** — PostgreSQL schema for shard registry and views  
✅ **Integration Tests** — Test suite for all components  
✅ **Documentation** — Design docs, implementation guide, runbooks

---

## Key Documents

| Document | Purpose |
|----------|---------|
| [DATABASE_SCALING_ARCHITECTURE.md](./DATABASE_SCALING_ARCHITECTURE.md) | High-level design, problem analysis, architecture overview |
| [DATABASE_SCALING_IMPLEMENTATION_GUIDE.md](./DATABASE_SCALING_IMPLEMENTATION_GUIDE.md) | Detailed implementation, configuration, deployment steps |
| [src/database/read_replica_router.rs](./src/database/read_replica_router.rs) | Read replica routing with health checking |
| [src/database/shard_manager.rs](./src/database/shard_manager.rs) | Logical sharding with hot-reload |
| [src/database/write_isolation.rs](./src/database/write_isolation.rs) | Write operation isolation with retry logic |
| [src/database/ledger_cache.rs](./src/database/ledger_cache.rs) | Query caching + materialized views |
| [src/database/monitoring.rs](./src/database/monitoring.rs) | Prometheus metrics |

---

## Quick Start

### 1. Run Migrations

```bash
# Apply schema changes (shard registry, materialized views, indexes)
sqlx migrate run --database-url $DATABASE_URL

# Verify migrations applied
sqlx migrate info --database-url $DATABASE_URL
```

### 2. Initialize Shard Registry

```sql
-- Connect to coordinator database and insert shards
INSERT INTO shard_registry (
    shard_id, corridor_id, week_id,
    primary_dsn, replica_dsns,
    status, max_connections, weight
) VALUES
    (0, 'NG', 202601,
     'postgres://ng-w1-primary:5432/aframp',
     ARRAY['postgres://ng-w1-replica1:5432/aframp'],
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

### 3. Environment Configuration

```bash
# Copy to your .env or deployment configuration
export DATABASE_COORDINATOR_URL=postgres://coordinator:5432/aframp
export DB_REPLICA_LAG_THRESHOLD_MS=5000
export DB_HEALTH_CHECK_INTERVAL_SECS=30
export SHARD_REGISTRY_REFRESH_INTERVAL_SECS=60

# Write isolation pools
export DB_SETTLEMENT_POOL_MAX_CONNECTIONS=24
export DB_AUDIT_POOL_MAX_CONNECTIONS=16
export DB_ANALYTICS_POOL_MAX_CONNECTIONS=32
```

### 4. Application Integration

```rust
// In your main.rs or initialization code
use crate::database::{
    read_replica_router::ReadReplicaRouter,
    shard_manager::ShardManager,
    write_isolation::WriteIsolationManager,
    ledger_cache::{LedgerQueryAccelerator, QueryResultCache, MaterializedViewManager},
    monitoring::init_metrics,
};

// Initialize at startup
init_metrics()?;

let shard_mgr = Arc::new(
    ShardManager::new(
        Arc::new(coordinator_pool),
        Duration::from_secs(60),
    )
    .await?
);

let read_router = Arc::new(
    ReadReplicaRouter::new(
        Arc::new(primary_pool),
        replica_dsns,
        None,
        5000,
    )
    .await?
);

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
```

### 5. Use the Components

```rust
// Route a read query to replica (eventual consistency)
let result = read_router.execute_read(
    |pool| sqlx::query("SELECT * FROM transactions WHERE id = $1").bind(id),
    ConsistencyLevel::Eventual,
    Some("shard_0"),
).await?;

// Route a settlement write (with isolation + retries)
write_mgr
    .execute_write(WriteOperationType::Settlement, |tx| {
        Box::pin(async {
            sqlx::query("INSERT INTO settlement_batches ...")
                .execute(&mut **tx)
                .await
                .map_err(|e| DatabaseError::from_sqlx(e))
        })
    })
    .await?;

// Get settlement statistics (cached)
let summary = accelerator
    .settlement_summary_by_corridor_week("NG", 202601)
    .await?;
```

---

## Architecture Summary

### High-Level Design

```
┌─────────────────────────────────────────┐
│         Application Layer               │
│  (Transaction Service, Settlement)      │
└────────────────┬────────────────────────┘
                 │
    ┌────────────┴────────────┐
    ▼                         ▼
┌──────────────────┐   ┌──────────────────┐
│  Read Router     │   │  Write Manager   │
│ (ConsistencyLevel│   │ (Operation Type) │
│ + Failover)      │   │ (Circuit Breaker)│
└────────┬─────────┘   └────────┬─────────┘
         │                      │
    ┌────┴──────────────────────┴────┐
    │   Shard Manager                │
    │ (Route by corridor_id + week)  │
    └────┬──────────────────────┬────┘
         │                      │
    ┌────▼──────┐         ┌────▼──────┐
    │ Replicas  │         │ Primaries │
    │ (Read ⓡ)  │         │ (Write ✎) │
    └───────────┘         └───────────┘
```

### Data Flow

**Read Query** → ConsistencyLevel → ReadRouter → HealthCheck → Replica/Primary → Cache → Result

**Write Operation** → WriteOperationType → WriteManager → Isolation Level → Shard → Primary → Retry/Backoff → Commit/Rollback

---

## Performance Targets

| Operation | p99 Latency | Throughput | Availability |
|-----------|-------------|-----------|--------------|
| Read (eventual) | <100ms | Unlimited (replicas) | 99.9% (3 replicas) |
| Read (consistent) | <200ms | Limited (primary) | 99.5% (1 primary) |
| Write (settlement) | <500ms | ~100 TPS/primary | 99.99% (serializable) |
| Replica lag | <5s | N/A (async) | 99.9% uptime |
| Query cache hit | <10ms | N/A (in-mem) | 100% (local) |

---

## Monitoring

### Key Metrics

View in Prometheus/Grafana:

```prometheus
# Read latency by shard
histogram_quantile(0.99, rate(db_read_latency_seconds_bucket[5m])) by (shard_id)

# Replica lag alert
db_replica_lag_seconds > 5 OR db_replica_health == 0

# Connection pool saturation
db_connection_pool_utilization > 0.8

# Settlement write performance
rate(db_settlement_write_latency_seconds_sum[5m]) / rate(db_settlement_write_latency_seconds_count[5m])
```

### Dashboards

- **Shard Health**: Per-shard read/write latency, replica lag, active connections
- **Write Operations**: Settlement vs. analytics latency, retry rates, serialization conflicts
- **Read Replicas**: Lag trends, failover events, query distribution
- **Cache Performance**: Hit rate, evictions, query acceleration stats

---

## Deployment Phases

### Phase 1: Foundation ✅
- [x] Schema migration (shard registry + views)
- [x] Read replica router implementation
- [x] Shard manager implementation
- [x] Write isolation manager
- [x] Query accelerator
- [x] Monitoring + metrics

### Phase 2: Integration (In Progress)
- [ ] Connect transaction repository to router
- [ ] Connect settlement service to write manager
- [ ] Integrate cache into analytics service
- [ ] Deploy materialized view refresh jobs
- [ ] Load test at 300+ TPS

### Phase 3: Production Rollout
- [ ] Canary deployment (10% traffic)
- [ ] Monitor replica lag + failover events
- [ ] Gradual read traffic migration (50% → 100%)
- [ ] Enable write isolation for settlement
- [ ] Validate settlement accuracy

### Phase 4: Optimization
- [ ] Tune pool sizes based on observed load
- [ ] Add cross-shard transaction support (if needed)
- [ ] Optimize query plans for materialized views
- [ ] Add multi-region failover

---

## Troubleshooting

### Replica Lag Spike

```sql
-- Check replica lag
SELECT 
    now() - pg_last_xact_replay_timestamp() as replication_lag,
    COALESCE(pg_last_wal_receive_lsn(), '0/0') as receive_lsn,
    pg_last_wal_replay_lsn() as replay_lsn;

-- Check WAL sender
SELECT * FROM pg_stat_replication;

-- Increase wal_keep_size on primary if lag persistent
ALTER SYSTEM SET wal_keep_size = '5GB';
SELECT pg_reload_conf();
```

### Circuit Breaker Open

```bash
# Check circuit breaker metrics
curl http://localhost:8080/metrics | grep circuit_breaker

# Reset circuit breaker (admin only)
curl -X POST http://localhost:8080/admin/reset-circuit-breaker \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

### Connection Pool Exhaustion

```bash
# Increase pool size
export DB_SETTLEMENT_POOL_MAX_CONNECTIONS=32  # from 24

# Or reduce idle timeout
export DB_CONNECTION_IDLE_TIMEOUT_SECS=30  # from 60
```

---

## Contributing

When adding new database operations:

1. **Determine consistency level**: `ConsistencyLevel::Eventual` for reads, or `ConsistencyLevel::ReadYourWrites` if dependent on recent writes
2. **Determine operation type**: `WriteOperationType::Settlement` for critical, `WriteOperationType::Analytics` for bufferable
3. **Use the appropriate router**:
   ```rust
   // For reads
   let result = read_router.execute_read(query_fn, ConsistencyLevel::Eventual, Some(shard_key)).await?;
   
   // For writes
   write_mgr.execute_write(WriteOperationType::Settlement, |tx| { ... }).await?;
   ```
4. **Record metrics**:
   ```rust
   MetricRecorder::record_read_operation("shard_0", "eventual", elapsed_secs);
   ```

---

## Support & Questions

- **Architecture Questions**: See [DATABASE_SCALING_ARCHITECTURE.md](./DATABASE_SCALING_ARCHITECTURE.md)
- **Implementation Details**: See [DATABASE_SCALING_IMPLEMENTATION_GUIDE.md](./DATABASE_SCALING_IMPLEMENTATION_GUIDE.md)
- **Code Examples**: See [tests/database_scaling_integration.rs](./tests/database_scaling_integration.rs)
- **Issues/Bugs**: Create issue with `database-scaling` label

---

## License

Part of the Aframp project. See LICENSE.md

