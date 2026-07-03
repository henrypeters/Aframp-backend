# Aframp Production Operations Guide

## Overview

This guide covers the production operations infrastructure for the Aframp payment network, including automated database maintenance, performance profiling, financial reconciliation, and log management systems.

## Table of Contents

1. [Database Maintenance & Partitioning](#database-maintenance--partitioning)
2. [Performance Profiling](#performance-profiling)
3. [Financial Reconciliation](#financial-reconciliation)
4. [Log Management](#log-management)
5. [Monitoring & Alerting](#monitoring--alerting)
6. [Performance Testing](#performance-testing)

---

## Database Maintenance & Partitioning

### Overview

Automated partition management for high-volume metrics tables with optimized autovacuum settings and cold storage migration.

### Key Features

- **Declarative Partitioning**: Time-series partitioning for `risk_exposure_snapshots` and `partner_performance_logs`
- **Automated Partition Creation**: Daily jobs create future partitions 7 days ahead
- **Cold Storage Migration**: Archives partitions older than 90 days to S3 with SHA-256 integrity proofs
- **Optimized Autovacuum**: Tuned settings for high-throughput transactional tables

### Configuration

```sql
-- Create future partitions manually
SELECT create_future_partitions('risk_exposure_snapshots', 7);

-- Get archival candidates
SELECT * FROM get_archival_candidates('partner_performance_logs', 90);

-- Check partition health
SELECT * FROM v_partition_health;
```

### Monitoring

```sql
-- View autovacuum activity
SELECT * FROM v_autovacuum_activity;

-- Check partition management logs
SELECT * FROM partition_management_log
WHERE created_at > NOW() - INTERVAL '24 hours'
ORDER BY created_at DESC;
```

### Performance Targets

- **P99 Database Locks**: < 5ms
- **Autovacuum Frequency**: Every 24 hours for active tables
- **Partition Creation**: 100% success rate
- **Table Bloat**: < 20% dead rows

---

## Performance Profiling

### Overview

Continuous memory and CPU profiling infrastructure for production workloads using custom tracking and alternative memory allocators.

### Memory Allocators

The system supports three memory allocators:

1. **System Allocator** (default): Standard OS allocator
2. **jemalloc**: High-performance allocator optimized for concurrent workloads
3. **mimalloc**: Microsoft allocator with excellent fragmentation characteristics

#### Building with Alternative Allocators

```bash
# Build with jemalloc
cargo build --release --features jemalloc,database

# Build with mimalloc
cargo build --release --features mimalloc,database
```

### Profiling API Endpoints

```bash
# Get current memory statistics
curl http://localhost:8000/profiling/memory

# Get allocation hot-spots
curl http://localhost:8000/profiling/hotspots

# Get overall profiling status
curl http://localhost:8000/profiling/status

# Toggle profiling on/off
curl -X POST http://localhost:8000/profiling/toggle \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}'

# Reset peak memory tracking
curl -X POST http://localhost:8000/profiling/reset
```

### Memory Tracking

The profiling system tracks:

- Current heap allocation in bytes
- Peak heap allocation
- Total allocations/deallocations count
- Allocation hot-spots by function
- Active allocations (leaks detection)

### Performance Targets

- **Memory Baseline**: Flat under sustained load
- **Heap Fragmentation**: < 30%
- **Allocation Rate**: < 100K allocations/sec for steady state

---

## Financial Reconciliation

### Overview

Hourly reconciliation system comparing internal ledger with on-chain Stellar account states, with automated circuit-breaker safety controls.

### Architecture

```
┌─────────────────┐
│ Reconciliation  │
│     Worker      │◄──── Every hour
└────────┬────────┘
         │
         ├─► Query Internal Ledger
         ├─► Query Stellar Account
         ├─► Calculate Drift
         ├─► Verify Transactions
         └─► Check Circuit Breakers
```

### Circuit Breakers

Circuit breakers automatically trip when drift exceeds thresholds:

- **Default Threshold**: 5 XLM (50,000,000 stroops) or 0.5%
- **Response Time**: < 500ms to block operations
- **Auto-Recovery**: Optional, configurable per corridor

#### Managing Circuit Breakers

```sql
-- Check circuit breaker status
SELECT * FROM v_circuit_breaker_status;

-- Manually reset a circuit breaker
SELECT reset_circuit_breaker(
    'NGN_CORRIDOR',
    '123e4567-e89b-12d3-a456-426614174000'::UUID, -- operator_id
    'Drift resolved after manual verification'
);

-- Check if operations are blocked
SELECT is_circuit_breaker_tripped('GLOBAL_RECONCILIATION');
```

### Reconciliation Dashboard

```sql
-- View recent reconciliation results
SELECT * FROM v_reconciliation_dashboard
ORDER BY hour DESC
LIMIT 24;

-- Check for unreconciled snapshots
SELECT * FROM reconciliation_ledger_snaps
WHERE NOT is_reconciled
AND snapshot_time > NOW() - INTERVAL '24 hours';

-- Audit transaction discrepancies
SELECT * FROM reconciliation_transaction_audit
WHERE discrepancy_type IS NOT NULL
ORDER BY created_at DESC;
```

### Performance Targets

- **Reconciliation Speed**: 50K transactions verified in < 30s
- **Drift Tolerance**: < 5 XLM absolute or 0.5% relative
- **Circuit Breaker Response**: < 500ms

---

## Log Management

### Overview

Automated log aggregation, rotation, and archival using Vector for streaming and custom scripts for rotation.

### Vector Configuration

Vector is deployed as a DaemonSet on all Kubernetes nodes to:

- Collect logs from all pods
- Parse JSON structured logs
- Mask PII (emails, phones, API keys)
- Route logs to multiple destinations
- Provide real-time metrics

#### Destinations

1. **CloudWatch**: Real-time log streaming
2. **S3**: Long-term archival (compressed, encrypted)
3. **Elasticsearch**: Search and analytics
4. **Slack**: Critical error alerts

### Log Rotation

Daily log rotation with:

- **PII Masking**: Automatic removal of sensitive data
- **Compression**: gzip level 9 (typical 70-90% reduction)
- **Encryption**: AES-256-GCM with KMS key
- **S3 Upload**: Intelligent tiering with SHA-256 integrity proofs

#### Running Log Rotation

```bash
# Manual rotation
./scripts/log-rotation.sh

# Configure via cron (daily at 2 AM)
0 2 * * * /path/to/scripts/log-rotation.sh
```

### Configuration

```bash
# Environment variables
export LOG_DIR=/var/log/aframp
export ARCHIVE_DIR=/var/log/aframp/archive
export RETENTION_DAYS=90
export S3_BUCKET=s3://aframp-logs-archive
export KMS_KEY_ID=alias/aframp-logs
```

### Performance Targets

- **Log Delivery Rate**: 100% (no dropped events)
- **Processing Latency**: < 5 seconds end-to-end
- **Rotation Time**: < 30 minutes for daily logs

---

## Monitoring & Alerting

### Prometheus Alerting Rules

Comprehensive alerting for:

- **Database**: Lock contention, partition failures, autovacuum stalls
- **Memory**: Leak detection, high utilization, fragmentation
- **Reconciliation**: Drift detection, circuit breaker trips, failures
- **Logs**: Delivery rate, processing backlog, rotation failures
- **Performance**: P99 latency, error rates, slow queries

### Grafana Dashboard

The Production Operations Dashboard provides real-time visibility into:

- Memory usage trends and allocation rates
- Database partition health and autovacuum activity
- Reconciliation drift and circuit breaker status
- Log delivery rates and critical error volumes
- P99 latency by endpoint
- Top memory allocation hot-spots

**Access**: `http://grafana.aframp.internal/d/production-operations`

### Key Metrics

```promql
# Memory leak detection
(avg_over_time(process_resident_memory_bytes[1h]) - 
 avg_over_time(process_resident_memory_bytes[1h] offset 1h)) > 104857600

# Reconciliation drift
abs(reconciliation_balance_drift_stroops) > 50000000

# Log delivery rate
rate(vector_events_out_total[5m]) / rate(vector_events_in_total[5m]) < 0.99

# P99 latency
histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m])) > 1.0
```

---

## Performance Testing

### Automated Performance Drills

High-load performance testing to verify memory stability and latency targets.

#### Running a Performance Drill

```bash
# Standard drill (1000 concurrent users, 5 minutes)
./scripts/performance-drill.sh

# Custom configuration
API_BASE_URL=https://api.aframp.com \
DRILL_DURATION=600 \
CONCURRENT_USERS=2000 \
REQUESTS_PER_SECOND=1000 \
./scripts/performance-drill.sh
```

#### What It Tests

- Memory stability under sustained load
- P99 latency under concurrency
- Error rates at high throughput
- System resource utilization

### Daily Performance Reports

Automated daily reports summarizing:

- P99/P95/P50 latency metrics
- Error rates and total request volumes
- Memory usage (current and peak)
- Database query performance
- Reconciliation success rates
- Circuit breaker trip counts
- Log delivery rates

#### Running Daily Report

```bash
# Generate report
./scripts/daily-performance-report.sh

# With Slack notifications
SLACK_WEBHOOK=https://hooks.slack.com/services/YOUR/WEBHOOK/URL \
./scripts/daily-performance-report.sh

# Configure via cron (daily at 6 AM)
0 6 * * * /path/to/scripts/daily-performance-report.sh
```

### Health Score Calculation

The health score (0-100) is calculated based on:

- **P99 Latency**: -20 points if > 1s
- **Error Rate**: -25 points if > 1%
- **Memory Growth**: -15 points if > 20% increase
- **Reconciliation Failures**: -20 points if any
- **Circuit Breaker Trips**: -20 points if any
- **Log Delivery**: -10 points if < 99%

**Interpretation**:

- **95-100**: Excellent health
- **80-94**: Good health, minor optimizations
- **60-79**: Fair health, attention required
- **0-59**: Poor health, immediate investigation

---

## Deployment Checklist

### Initial Setup

1. ✅ Apply database migrations
2. ✅ Configure alternative memory allocator (optional)
3. ✅ Deploy Vector DaemonSet for log collection
4. ✅ Set up S3 buckets for log archival
5. ✅ Configure Prometheus alerting rules
6. ✅ Import Grafana dashboard
7. ✅ Set up Slack webhooks for notifications
8. ✅ Configure cron jobs for automation

### Verification

```bash
# Test database partitioning
psql -c "SELECT create_future_partitions('risk_exposure_snapshots', 1);"

# Test profiling API
curl http://localhost:8000/profiling/status | jq

# Test reconciliation worker (manual run)
# Execute via admin API or database function

# Test log rotation
./scripts/log-rotation.sh

# Run performance drill
./scripts/performance-drill.sh

# Generate performance report
./scripts/daily-performance-report.sh
```

---

## Troubleshooting

### Memory Leak Detected

1. Check profiling hot-spots: `curl /profiling/hotspots`
2. Review allocation patterns in Grafana
3. Consider switching to jemalloc allocator
4. Investigate top functions with high byte counts

### Reconciliation Drift

1. Check circuit breaker status: `SELECT * FROM v_circuit_breaker_status`
2. Review recent snapshots: `SELECT * FROM reconciliation_ledger_snaps ORDER BY snapshot_time DESC LIMIT 10`
3. Verify Stellar connectivity
4. Check for pending transactions
5. Manually reset circuit breaker if drift is resolved

### High Database Lock Contention

1. Check partition health: `SELECT * FROM v_partition_health`
2. Review autovacuum activity: `SELECT * FROM v_autovacuum_activity`
3. Run manual reindex if needed: `SELECT reindex_bloated_indexes(30.0)`
4. Consider adjusting autovacuum settings

### Log Delivery Issues

1. Check Vector pod status: `kubectl get pods -l app=vector`
2. Review Vector metrics: `curl http://vector:9598/metrics`
3. Verify S3 bucket permissions
4. Check CloudWatch log group configuration

---

## Support

For operational issues:

- **Slack**: #aframp-operations
- **PagerDuty**: Critical alerts auto-escalate
- **Runbooks**: `/docs/runbooks/`
- **Dashboards**: https://grafana.aframp.internal

---

**Last Updated**: 2027-06-30  
**Version**: 1.0.0
