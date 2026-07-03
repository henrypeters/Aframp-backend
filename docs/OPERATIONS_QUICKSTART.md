# Production Operations Quick Start

## TL;DR

Automated operational infrastructure for database maintenance, performance profiling, financial reconciliation, and log management.

## Installation (5 minutes)

### 1. Database Setup

```bash
# Apply migrations
psql $DATABASE_URL -f migrations/20270630000000_automated_maintenance_partitioning.sql
psql $DATABASE_URL -f migrations/20270630000001_financial_reconciliation.sql

# Verify setup
psql $DATABASE_URL -c "SELECT * FROM v_partition_health LIMIT 5;"
psql $DATABASE_URL -c "SELECT * FROM v_circuit_breaker_status;"
```

### 2. Enable Profiling (Optional - Better Performance)

```bash
# Rebuild with jemalloc for better memory performance
cargo build --release --features jemalloc,database

# Or with mimalloc
cargo build --release --features mimalloc,database
```

### 3. Deploy Log Management

```bash
# Create namespace
kubectl create namespace aframp-production

# Deploy Vector
kubectl apply -f k8s/logging/vector-configmap.yaml
kubectl apply -f k8s/logging/vector-daemonset.yaml

# Verify
kubectl get pods -n aframp-production -l app=vector
```

### 4. Configure Monitoring

```bash
# Add Prometheus rules
kubectl apply -f monitoring/prometheus/rules/operations.yml

# Import Grafana dashboard
# Navigate to Grafana → Dashboards → Import
# Upload: monitoring/grafana/production-operations-dashboard.json
```

### 5. Schedule Automation

```bash
# Make scripts executable
chmod +x scripts/log-rotation.sh
chmod +x scripts/daily-performance-report.sh
chmod +x scripts/performance-drill.sh

# Add to crontab
crontab -e
```

Add these lines:

```cron
# Log rotation - daily at 2 AM
0 2 * * * /path/to/scripts/log-rotation.sh

# Performance report - daily at 6 AM
0 6 * * * SLACK_WEBHOOK=<your-webhook> /path/to/scripts/daily-performance-report.sh

# Performance drill - weekly on Sunday at 3 AM
0 3 * * 0 /path/to/scripts/performance-drill.sh
```

## Verification (2 minutes)

```bash
# 1. Check profiling is working
curl http://localhost:8000/profiling/status | jq

# 2. Verify partitions are created
psql $DATABASE_URL -c "
SELECT COUNT(*) as partition_count 
FROM pg_tables 
WHERE tablename LIKE '%_snapshots_%' 
  OR tablename LIKE '%_logs_%';"

# 3. Check circuit breakers
psql $DATABASE_URL -c "SELECT circuit_name, is_tripped FROM reconciliation_circuit_breaker;"

# 4. Test log rotation
./scripts/log-rotation.sh

# 5. Generate test performance report
./scripts/daily-performance-report.sh
```

## Key Endpoints

### Profiling API

```bash
# Memory stats
curl http://localhost:8000/profiling/memory

# Hot-spots
curl http://localhost:8000/profiling/hotspots

# Overall status
curl http://localhost:8000/profiling/status
```

### Monitoring

- **Grafana Dashboard**: http://grafana/d/production-operations
- **Prometheus Alerts**: http://prometheus:9090/alerts
- **Vector Metrics**: http://vector:9598/metrics

## Common Tasks

### Reset Circuit Breaker

```sql
SELECT reset_circuit_breaker(
    'NGN_CORRIDOR',
    'YOUR-USER-ID'::UUID,
    'Drift resolved after investigation'
);
```

### Check Recent Reconciliation

```sql
SELECT * FROM v_reconciliation_dashboard
ORDER BY hour DESC
LIMIT 24;
```

### View Partition Health

```sql
SELECT * FROM v_partition_health
WHERE dead_row_percentage > 20;
```

### Manual Performance Drill

```bash
API_BASE_URL=http://localhost:8000 \
CONCURRENT_USERS=500 \
DRILL_DURATION=180 \
./scripts/performance-drill.sh
```

## Alerting

Critical alerts auto-fire to:

- **Slack**: #aframp-operations
- **PagerDuty**: On-call engineer

Alert categories:

- 🔴 **Critical**: Memory leaks, circuit breakers, reconciliation failures
- 🟡 **Warning**: High latency, database bloat, log delivery issues

## Performance Targets

| Metric | Target | Alert Threshold |
|--------|--------|----------------|
| P99 Latency | < 1s | > 1s |
| Memory Growth | Flat baseline | > 10% increase/hour |
| Reconciliation Drift | < 5 XLM | > 5 XLM or 0.5% |
| Log Delivery | 100% | < 99% |
| Database Locks | < 5ms P99 | > 5ms |
| Error Rate | < 0.1% | > 1% |

## Troubleshooting

### Memory Leak

```bash
# Check hot-spots
curl http://localhost:8000/profiling/hotspots | jq '.[] | select(.total_bytes > 10000000)'

# Switch to jemalloc
cargo build --release --features jemalloc,database
```

### Reconciliation Drift

```sql
-- Check status
SELECT * FROM reconciliation_ledger_snaps 
WHERE NOT is_reconciled 
ORDER BY snapshot_time DESC LIMIT 5;

-- Manual reset (if false positive)
SELECT reset_circuit_breaker('GLOBAL_RECONCILIATION', 'YOUR-ID'::UUID, 'Manual override');
```

### Log Rotation Failed

```bash
# Check permissions
ls -la /var/log/aframp/

# Check S3 access
aws s3 ls s3://aframp-logs-archive/

# Manual rotation
LOG_DIR=/var/log/aframp ./scripts/log-rotation.sh
```

## Support

- **Documentation**: `/docs/PRODUCTION_OPERATIONS.md`
- **Slack**: #aframp-operations
- **On-call**: PagerDuty escalation

---

**Setup Time**: ~10 minutes  
**Maintenance**: Fully automated  
**Monitoring**: 24/7 automated alerts
