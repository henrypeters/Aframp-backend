# cNGN Database Scaling Architecture

**Issue**: Write-heavy saturation at scale across multiple African corridors  
**Scope**: Read replica routing, logical sharding for transaction ledgers, write isolation  
**Status**: In Development  
**Last Updated**: 2026-06-01

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Problem Statement](#problem-statement)
3. [Architecture Overview](#architecture-overview)
4. [Components](#components)
5. [Implementation Strategy](#implementation-strategy)
6. [Deployment & Operations](#deployment--operations)
7. [Monitoring & Observability](#monitoring--observability)
8. [Migration Path](#migration-path)

---

## Executive Summary

As cNGN scales across Nigeria, Ghana, Kenya, Senegal, and emerging corridors, transaction volumes create write contention on a single PostgreSQL primary. This architecture introduces:

1. **Read replica routing** within the Rust data layer (extends existing HA pool)
2. **Logical sharding framework** for transaction ledgers by corridor/corridor-week
3. **Write operation isolation** separating settlement paths from query-heavy analytics
4. **Query acceleration layer** for read-intensive operations (auditing, analytics, verification)

**Key Guarantees**:
- ✅ Write operations maintain ACID across all shards
- ✅ Read operations achieve <100ms p99 latency from replicas
- ✅ Settlement path isolated from analytics queries
- ✅ No application-level shard-aware logic (routing transparent via data layer)
- ✅ Hot shard addition without process restart

---

## Problem Statement

### Current Bottlenecks

| Bottleneck | Symptom | Root Cause |
|-----------|---------|-----------|
| Write saturation | Commit latency >500ms | All writes queue to single primary |
| Lock contention | Settlement conflicts | Concurrent settlement batch updates |
| Query storms | Replica lag >30s | Analytics scanning entire transaction table |
| Audit ledger I/O | Insert latency 100-200ms | Append-only table becomes hot spot |
| Corridor isolation | Cross-corridor queries | No data locality optimization |

### Transaction Volume Projections

```
Scenario: 3 corridors × 50 TPS baseline = 150 TPS steady
Peak load: 300 TPS (order confirmation surge)
Monthly transactions: ~400M
Annual growth: 8x → 3.2B transactions
```

### Read/Write Split (Observed)

- **Writes**: Settlement, compliance, KYA (20% of queries)
- **Reads**: Analytics, auditing, ledger verification (80% of queries)

---

## Architecture Overview

### High-Level Topology

```
                              ┌─────────────────┐
                              │   Application   │
                              │   (Rust Server) │
                              └────────┬────────┘
                                       │
                     ┌─────────────────┴─────────────────┐
                     ▼                                   ▼
           ┌─────────────────┐              ┌─────────────────┐
           │  Write Router   │              │  Read Router    │
           │  (Primary Pool) │              │  (Replica Pool) │
           └────────┬────────┘              └────────┬────────┘
                    │                               │
         ┌──────────┼──────────┬──────────┐        │
         ▼          ▼          ▼          ▼        │
    ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌──────────────┐
    │Shard 0  │ │Shard 1  │ │Shard 2  │ │Shard N+1     │
    │(NG-W1)  │ │(GH-W1)  │ │(KE-W1)  │ │(Geo-Locked)  │
    └────┬────┘ └────┬────┘ └────┬────┘ └──────┬───────┘
         │           │           │             │
    ┌────┴──────┐ ┌──┴───────┐ ┌┴────────┐ ┌──┴───────┐
    │   Primary │ │ Primary  │ │ Primary │ │ Primary  │
    │   (8 conn)│ │ (8 conn) │ │(8 conn) │ │(8 conn)  │
    └──┬────────┘ └──┬───────┘ └┬────────┘ └──┬───────┘
       │             │           │            │
     ┌─┴─┬───┐   ┌──┴─┬───┐   ┌─┴─┬───┐  ┌──┴─┬───┐
     │   │   │   │    │   │   │   │   │  │    │   │
    Replica Replica Replica Replica Replica Replica
    ×2      ×2      ×2      ×2
    (32 conn) (32 conn) (32 conn) (32 conn)

    Read routing: Round-robin → Replica, Fallback → Primary (if all down)
    Write routing: Always → Primary (sharding key determines which shard)
```

### Shard Key Strategy

**Primary Shard Key**: `corridor_id` (NG, GH, KE, SN, ...) + optional `week_id` for time-based splits

```
Transaction.shard_key = format!("{}_W{}", corridor_id, week_id)
Example: "NG_W202601" → routes to Nigeria Week-1 shard

Alternative (if single-corridor bottleneck):
  Sub-shard by merchant_id within corridor
  Merchants 0-50k → NG_M0
  Merchants 50k-100k → NG_M1
  (Requires re-hashing if merchant count grows)
```

### Write/Read Separation

**Write Operations** (route to primary):
- Settlement batch creation/update
- Transaction status updates
- KYA approvals/rejections
- Compliance rule violations
- Ledger entries (must be total-order)

**Read Operations** (route to replica):
- Analytics queries (trend analysis, conversion rates)
- Audit ledger verification
- Settlement reconciliation checks
- Customer transaction history
- Compliance monitoring

---

## Components

### 1. Enhanced Read Replica Router

**File**: `src/database/read_replica_router.rs`

Extends `ha_pool::HaPoolManager` with:

- **Weighted load balancing**: Assign load weights per replica based on observed latency
- **Consistent-read support**: Option to force primary reads for transactionally-consistent queries
- **Replica health tracking**: Automatic failover on replica lag >5s
- **Query classification**: Heuristic to detect read-only queries and route accordingly

```rust
pub struct ReadReplicaRouter {
    primary_pool: PgPool,
    replica_pool: Option<PgPool>,
    router: Arc<ha_pool::HaPoolManager>,
    replica_health: Arc<ReplicaHealthChecker>,
}

impl ReadReplicaRouter {
    /// Route a query based on consistency requirement
    pub async fn execute_query(
        &self,
        sql: &str,
        shard_key: &str,
        consistency: ConsistencyLevel,
    ) -> Result<Vec<Row>, DatabaseError> {
        match consistency {
            ConsistencyLevel::Eventual => self.route_to_replica(shard_key).await,
            ConsistencyLevel::ReadYourWrites => self.route_to_primary(shard_key).await,
        }
    }
}
```

### 2. Logical Sharding Framework

**File**: `src/database/shard_manager.rs`

Manages:

- **Shard registry**: PostgreSQL table tracking active shards, their status, and DSNs
- **Shard split operations**: Add new shard without downtime via "draining" state
- **Consistent hashing**: FNV-1a hash with virtual nodes for uniform distribution
- **Shard discovery**: Read registry on startup; poll for changes periodically

**Schema**:
```sql
CREATE TABLE shard_registry (
    shard_id INT PRIMARY KEY,
    corridor_id TEXT NOT NULL,
    week_id INT,
    primary_dsn TEXT NOT NULL,
    replica_dsns TEXT[] DEFAULT ARRAY[]::TEXT[],
    status TEXT CHECK (status IN ('active', 'draining', 'offline')),
    max_connections INT DEFAULT 8,
    weight INT DEFAULT 1,  -- for weighted load balancing
    created_at TIMESTAMPTZ DEFAULT NOW()
);
```

### 3. Write Operation Isolation

**File**: `src/database/write_isolation.rs`

Guarantees:

- **Settlement writes** use serializable transactions + explicit locks
- **Audit ledger writes** use append-only enforcement (immutable after insert)
- **Analytics writes** batched and isolated from settlement path

```rust
pub struct WriteIsolationManager {
    settlement_pool: Arc<PgPool>,  // Dedicated to settlement writes
    audit_pool: Arc<PgPool>,       // Dedicated to audit writes
    analytics_pool: Arc<PgPool>,   // Buffered analytics writes
}

impl WriteIsolationManager {
    /// Isolated settlement transaction
    pub async fn execute_settlement_write(
        &self,
        tx_fn: impl FnOnce(&mut PgTransaction) -> BoxFuture<Result<()>>,
    ) -> Result<()> {
        let mut tx = self.settlement_pool.begin().await?;
        tx.execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE").await?;
        tx_fn(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
```

### 4. Ledger Query Acceleration

**File**: `src/database/ledger_cache.rs`

Acceleration strategies:

- **Materialized views**: Pre-aggregate settlement summaries per corridor-week
- **Bloom filters**: Track which transactions exist before heavy scans
- **Query result caching**: Redis-backed cache for read-only ledger queries
- **Columnar indexes**: Create partial indexes for common filters (status, created_at)

```rust
pub struct LedgerQueryAccelerator {
    cache: Arc<redis::Client>,
    view_manager: MaterializedViewManager,
}

impl LedgerQueryAccelerator {
    /// Get transaction count for corridor-week, using cache + view
    pub async fn transaction_count_by_corridor_week(
        &self,
        corridor_id: &str,
        week_id: i32,
    ) -> Result<i64> {
        let cache_key = format!("txn_count:{}:{}", corridor_id, week_id);
        
        // Try cache first (TTL 1 hour)
        if let Ok(count) = self.cache.get(&cache_key).await {
            return Ok(count);
        }
        
        // Query materialized view
        let count = self.view_manager
            .query_transaction_count(corridor_id, week_id)
            .await?;
        
        // Cache result
        self.cache.set_ex(&cache_key, count, 3600).await?;
        Ok(count)
    }
}
```

### 5. Connection Pool Optimization

**File**: `src/database/pool_optimizer.rs`

Tuning for write-heavy workloads:

- **Adaptive pool sizing**: Increase pool size if queue depth > threshold
- **Connection warming**: Pre-establish connections during low-traffic periods
- **Idle connection reaping**: Aggressive cleanup of idle connections
- **Write affinity**: Sticky routing to reuse connections for same shard

```rust
pub struct PoolOptimizer {
    write_pool_config: PoolConfig,
    read_pool_config: PoolConfig,
}

pub struct PoolConfig {
    pub max_connections: u32,      // write: 16-32, read: 64-128
    pub min_connections: u32,      // write: 8, read: 16
    pub queue_timeout: Duration,   // 5s
    pub idle_timeout: Duration,    // 5m
    pub max_lifetime: Duration,    // 30m
    pub acquire_timeout: Duration, // 10s
}
```

---

## Implementation Strategy

### Phase 1: Foundation (Week 1-2)

- [ ] Extend `ha_pool::HaPoolManager` with weighted replica routing
- [ ] Create shard registry table + Rust `ShardManager`
- [ ] Add `ConsistencyLevel` enum to query layer
- [ ] Implement `ReadReplicaRouter`

### Phase 2: Core (Week 3-4)

- [ ] Implement `WriteIsolationManager` with dedicated pools
- [ ] Create settlement write isolation logic
- [ ] Add audit ledger append-only enforcement
- [ ] Deploy per-corridor sharding config

### Phase 3: Acceleration (Week 5-6)

- [ ] Build materialized views for settlement summaries
- [ ] Implement `LedgerQueryAccelerator` with caching
- [ ] Add Bloom filter pre-checks
- [ ] Tune pool configuration for write-heavy workloads

### Phase 4: Operations (Week 7-8)

- [ ] Add comprehensive metrics + dashboards
- [ ] Create runbook for shard addition
- [ ] Implement replica health checker background task
- [ ] Load test at 300+ TPS

---

## Deployment & Operations

### Environment Variables

```bash
# Shard configuration
DATABASE_SHARD_CONFIG_JSON='{
  "shards": [
    {
      "shard_id": 0,
      "corridor_id": "NG",
      "week_id": 202601,
      "primary_url": "postgres://ng-w1-primary:5432/aframp",
      "replica_urls": ["postgres://ng-w1-replica1:5432/aframp", "postgres://ng-w1-replica2:5432/aframp"],
      "max_connections": 16
    }
  ],
  "checksum_interval_secs": 300
}'

# Pool tuning
DB_WRITE_MAX_CONNECTIONS=24
DB_WRITE_MIN_CONNECTIONS=8
DB_READ_MAX_CONNECTIONS=128
DB_READ_MIN_CONNECTIONS=16

# Read replica behavior
DB_READ_CONSISTENCY_MODE=eventual  # eventual | readyourwrites
DB_REPLICA_LAG_THRESHOLD_MS=5000   # Failover if lag exceeds
```

### Shard Configuration Evolution

**Week 1**: Single shard per corridor
```
shard_registry:
  0 | NG     | 202601 | active
  1 | GH     | 202601 | active
  2 | KE     | 202601 | active
```

**Week 5**: Time-based split (old week drains)
```
shard_registry:
  0 | NG | 202601 | draining (no new writes)
  1 | NG | 202602 | active   (new writes here)
  2 | GH | 202601 | active
  ...
```

**Week 10**: Merchant-ID sub-sharding (if NG bottleneck continues)
```
shard_registry:
  0 | NG_M0   | 202601 | active
  1 | NG_M1   | 202601 | active
  2 | NG_M2   | 202601 | active
  ...
```

---

## Monitoring & Observability

### Key Metrics

| Metric | Target | Alert Threshold |
|--------|--------|-----------------|
| Write latency (p99) | <200ms | >500ms |
| Read latency (p99) | <100ms | >300ms |
| Replica lag | <1s | >5s |
| Connection pool utilization | <60% | >80% |
| Shard balance | ±10% | >20% deviation |
| Transaction throughput | 300 TPS | <250 TPS drop |
| Settlement batch commit time | <500ms | >1000ms |

### Prometheus Metrics

```rust
// In read_replica_router.rs
lazy_static::lazy_static! {
    static ref READ_LATENCY_HISTOGRAM: prometheus::HistogramVec = 
        HistogramVec::new(
            HistogramOpts::new("db_read_latency_seconds", "Read query latency"),
            &["shard_id", "consistency"],
        ).unwrap();
    
    static ref WRITE_LATENCY_HISTOGRAM: prometheus::HistogramVec =
        HistogramVec::new(
            HistogramOpts::new("db_write_latency_seconds", "Write operation latency"),
            &["operation_type", "shard_id"],
        ).unwrap();
}
```

### Dashboards

**Dashboard 1: Shard Health**
- Per-shard write/read latency (p50, p95, p99)
- Replica lag trend (graph)
- Connection pool saturation per shard
- Transaction count by corridor

**Dashboard 2: Write Isolation**
- Settlement batch commit time
- Lock contention events
- Transaction rollback rate
- Serialization conflict count

**Dashboard 3: Read Performance**
- Query latency by type (analytics, audit, verification)
- Replica vs primary routing split
- Cache hit rate (ledger queries)
- Slow query log (>1s)

---

## Migration Path

### Step 1: Deploy Infrastructure (without changing routing)

1. Provision read replicas for each shard
2. Deploy `ReadReplicaRouter` (but route all to primary for now)
3. Create shard registry table
4. Initialize `ShardManager`

**Risk**: Low (no traffic changes)

### Step 2: Gradual Read Traffic Migration

1. Enable read replica routing for analytics queries only (ConsistencyLevel::Eventual)
2. Monitor replica lag + query latency
3. Expand to audit ledger queries
4. Use feature flags to control rollout (e.g., 10% → 50% → 100%)

**Risk**: Medium (eventual consistency may expose staleness bugs)

### Step 3: Write Isolation

1. Enable settlement writes to dedicated pool
2. Add serializable transaction enforcement
3. Test under load at 200+ TPS

**Risk**: Medium (serialization conflicts possible at high load)

### Step 4: Ledger Acceleration

1. Deploy materialized views (non-blocking)
2. Enable caching for ledger queries
3. Monitor cache hit rate

**Risk**: Low (read-only feature, no side effects)

### Rollback Plan

**If replica lag exceeds 10s**:
```rust
// Automatic failover in ReplicaHealthChecker
if replica_lag_ms > 10000 {
    mark_replica_unhealthy(replica_id);
    route_all_reads_to_primary();
    alert!("HIGH_REPLICA_LAG_FAILOVER");
}
```

**If write latency exceeds 1s**:
```rust
// Fall back to single primary (disable sharding)
if write_latency_p99_ms > 1000 && shard_count > 1 {
    disable_sharding();
    route_all_writes_to_coordinator();
    alert!("WRITE_LATENCY_SPIKE_SHARDING_DISABLED");
}
```

---

## Related Issues

- **#423**: HA Pool Manager (foundation for this work)
- **#347**: Distributed tracing for latency observability
- **#104**: Multi-region deployment coordination

---

## Appendix: Schema Changes

### 1. Shard Registry Table

```sql
CREATE TABLE shard_registry (
    shard_id INT PRIMARY KEY,
    corridor_id TEXT NOT NULL,
    week_id INT,
    primary_dsn TEXT NOT NULL,
    replica_dsns TEXT[] DEFAULT ARRAY[]::TEXT[],
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'draining', 'offline')),
    max_connections INT DEFAULT 8,
    weight INT DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_shard_registry_status ON shard_registry(status);
CREATE INDEX idx_shard_registry_corridor ON shard_registry(corridor_id, week_id);
```

### 2. Settlement Summaries Materialized View

```sql
CREATE MATERIALIZED VIEW settlement_summaries_by_week AS
SELECT
    corridor_id,
    EXTRACT(YEAR FROM created_at)::INT * 100 + EXTRACT(WEEK FROM created_at)::INT as week_id,
    SUM(gross_amount) as total_gross,
    SUM(platform_fee) as total_fees,
    COUNT(*) as batch_count,
    COUNT(CASE WHEN status = 'settled' THEN 1 END) as settled_count
FROM settlement_batches
GROUP BY corridor_id, week_id;

CREATE UNIQUE INDEX idx_settlement_summaries_unique 
ON settlement_summaries_by_week(corridor_id, week_id);
```

