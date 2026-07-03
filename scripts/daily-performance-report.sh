#!/usr/bin/env bash
# ============================================================================
# Daily Performance Report Generator
# ============================================================================
# Automated P99 execution summaries for engineering notification channels
# ============================================================================

set -euo pipefail

# Configuration
PROMETHEUS_URL="${PROMETHEUS_URL:-http://localhost:9090}"
SLACK_WEBHOOK="${SLACK_WEBHOOK:-}"
REPORT_DIR="${REPORT_DIR:-./reports/daily}"
LOOKBACK_HOURS="${LOOKBACK_HOURS:-24}"
TIMESTAMP=$(date '+%Y-%m-%d')

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*" >&2
}

# Create report directory
mkdir -p "$REPORT_DIR"

# ============================================================================
# Query Prometheus Metrics
# ============================================================================
query_prometheus() {
    local query="$1"
    local result
    
    result=$(curl -sG --data-urlencode "query=$query" \
        "${PROMETHEUS_URL}/api/v1/query" | jq -r '.data.result[0].value[1]' 2>/dev/null || echo "0")
    
    echo "$result"
}

query_prometheus_range() {
    local query="$1"
    local step="${2:-60}"
    
    local end_time=$(date +%s)
    local start_time=$((end_time - LOOKBACK_HOURS * 3600))
    
    curl -sG \
        --data-urlencode "query=$query" \
        --data-urlencode "start=$start_time" \
        --data-urlencode "end=$end_time" \
        --data-urlencode "step=$step" \
        "${PROMETHEUS_URL}/api/v1/query_range" | jq -r '.data.result'
}

# ============================================================================
# Collect Performance Metrics
# ============================================================================
collect_metrics() {
    log_info "Collecting performance metrics for last ${LOOKBACK_HOURS} hours..."
    
    # P99 Latency
    P99_LATENCY=$(query_prometheus \
        "histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[${LOOKBACK_HOURS}h]))")
    
    # P95 Latency
    P95_LATENCY=$(query_prometheus \
        "histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[${LOOKBACK_HOURS}h]))")
    
    # P50 Latency (Median)
    P50_LATENCY=$(query_prometheus \
        "histogram_quantile(0.50, rate(http_request_duration_seconds_bucket[${LOOKBACK_HOURS}h]))")
    
    # Error Rate
    ERROR_RATE=$(query_prometheus \
        "rate(http_requests_total{status=~\"5..\"}[${LOOKBACK_HOURS}h]) / rate(http_requests_total[${LOOKBACK_HOURS}h])")
    
    # Total Requests
    TOTAL_REQUESTS=$(query_prometheus \
        "sum(increase(http_requests_total[${LOOKBACK_HOURS}h]))")
    
    # Memory Usage (Current)
    CURRENT_MEMORY_MB=$(query_prometheus \
        "process_resident_memory_bytes{job=\"aframp-backend\"} / 1048576")
    
    # Memory Usage (Peak)
    PEAK_MEMORY_MB=$(query_prometheus \
        "max_over_time(process_resident_memory_bytes{job=\"aframp-backend\"}[${LOOKBACK_HOURS}h]) / 1048576")
    
    # Database P99 Query Time
    DB_P99_QUERY_TIME=$(query_prometheus \
        "histogram_quantile(0.99, rate(database_query_duration_seconds_bucket[${LOOKBACK_HOURS}h]))")
    
    # Reconciliation Stats
    RECONCILIATION_RUNS=$(query_prometheus \
        "sum(increase(reconciliation_runs_total[${LOOKBACK_HOURS}h]))")
    
    RECONCILIATION_FAILURES=$(query_prometheus \
        "sum(increase(reconciliation_failures_total[${LOOKBACK_HOURS}h]))")
    
    AVG_DRIFT_STROOPS=$(query_prometheus \
        "avg_over_time(abs(reconciliation_balance_drift_stroops)[${LOOKBACK_HOURS}h])")
    
    # Circuit Breaker Trips
    CIRCUIT_BREAKER_TRIPS=$(query_prometheus \
        "sum(increase(circuit_breaker_trips_total[${LOOKBACK_HOURS}h]))")
    
    # Log Delivery Rate
    LOG_DELIVERY_RATE=$(query_prometheus \
        "rate(vector_events_out_total[${LOOKBACK_HOURS}h]) / rate(vector_events_in_total[${LOOKBACK_HOURS}h])")
    
    log_info "Metrics collection complete"
}

# ============================================================================
# Generate Health Score
# ============================================================================
calculate_health_score() {
    local score=100
    
    # Deduct points for issues
    
    # P99 latency > 1s: -20 points
    if (( $(echo "$P99_LATENCY > 1.0" | bc -l) )); then
        score=$((score - 20))
    fi
    
    # Error rate > 1%: -25 points
    if (( $(echo "$ERROR_RATE > 0.01" | bc -l) )); then
        score=$((score - 25))
    fi
    
    # Memory increase > 20%: -15 points
    local memory_increase_pct=$(echo "scale=2; ($PEAK_MEMORY_MB / $CURRENT_MEMORY_MB - 1) * 100" | bc)
    if (( $(echo "$memory_increase_pct > 20" | bc -l) )); then
        score=$((score - 15))
    fi
    
    # Reconciliation failures: -20 points
    if (( $(echo "$RECONCILIATION_FAILURES > 0" | bc -l) )); then
        score=$((score - 20))
    fi
    
    # Circuit breaker trips: -20 points
    if (( $(echo "$CIRCUIT_BREAKER_TRIPS > 0" | bc -l) )); then
        score=$((score - 20))
    fi
    
    # Log delivery rate < 99%: -10 points
    if (( $(echo "$LOG_DELIVERY_RATE < 0.99" | bc -l) )); then
        score=$((score - 10))
    fi
    
    # Ensure score doesn't go below 0
    if [ $score -lt 0 ]; then
        score=0
    fi
    
    echo "$score"
}

# ============================================================================
# Generate Text Report
# ============================================================================
generate_text_report() {
    local report_file="${REPORT_DIR}/report_${TIMESTAMP}.txt"
    local health_score=$(calculate_health_score)
    
    cat > "$report_file" <<EOF
================================================================================
Aframp Production Performance Report
Date: $TIMESTAMP
Period: Last ${LOOKBACK_HOURS} hours
================================================================================

HEALTH SCORE: ${health_score}/100

API PERFORMANCE
---------------
• P99 Latency:        $(printf "%.3f" "$P99_LATENCY")s
• P95 Latency:        $(printf "%.3f" "$P95_LATENCY")s
• P50 Latency:        $(printf "%.3f" "$P50_LATENCY")s
• Error Rate:         $(printf "%.4f" "$ERROR_RATE")% ($(echo "$ERROR_RATE * 100" | bc)%)
• Total Requests:     $(printf "%.0f" "$TOTAL_REQUESTS")

MEMORY & RESOURCES
------------------
• Current Memory:     $(printf "%.2f" "$CURRENT_MEMORY_MB") MB
• Peak Memory:        $(printf "%.2f" "$PEAK_MEMORY_MB") MB
• Memory Increase:    $(echo "scale=2; ($PEAK_MEMORY_MB - $CURRENT_MEMORY_MB) / $CURRENT_MEMORY_MB * 100" | bc)%

DATABASE PERFORMANCE
--------------------
• P99 Query Time:     $(printf "%.3f" "$DB_P99_QUERY_TIME")s

RECONCILIATION
--------------
• Total Runs:         $(printf "%.0f" "$RECONCILIATION_RUNS")
• Failures:           $(printf "%.0f" "$RECONCILIATION_FAILURES")
• Avg Drift:          $(printf "%.0f" "$AVG_DRIFT_STROOPS") stroops

CIRCUIT BREAKERS
----------------
• Total Trips:        $(printf "%.0f" "$CIRCUIT_BREAKER_TRIPS")

LOG MANAGEMENT
--------------
• Delivery Rate:      $(printf "%.4f" "$LOG_DELIVERY_RATE") ($(echo "$LOG_DELIVERY_RATE * 100" | bc)%)

================================================================================
RECOMMENDATIONS
================================================================================
EOF
    
    # Add recommendations based on metrics
    {
        if (( $(echo "$P99_LATENCY > 1.0" | bc -l) )); then
            echo "⚠️  P99 latency exceeds 1s threshold - investigate slow endpoints"
        fi
        
        if (( $(echo "$ERROR_RATE > 0.01" | bc -l) )); then
            echo "⚠️  Error rate above 1% - review error logs and failure patterns"
        fi
        
        if (( $(echo "$RECONCILIATION_FAILURES > 0" | bc -l) )); then
            echo "⚠️  Reconciliation failures detected - verify Stellar connectivity"
        fi
        
        if (( $(echo "$CIRCUIT_BREAKER_TRIPS > 0" | bc -l) )); then
            echo "⚠️  Circuit breaker trips detected - review drift thresholds"
        fi
        
        if [ "$health_score" -ge 95 ]; then
            echo "✅ System health excellent - all metrics within target ranges"
        elif [ "$health_score" -ge 80 ]; then
            echo "✅ System health good - minor optimizations recommended"
        elif [ "$health_score" -ge 60 ]; then
            echo "⚠️  System health fair - attention required on flagged metrics"
        else
            echo "❌ System health poor - immediate investigation required"
        fi
    } >> "$report_file"
    
    echo "$report_file"
}

# ============================================================================
# Generate JSON Report
# ============================================================================
generate_json_report() {
    local report_file="${REPORT_DIR}/report_${TIMESTAMP}.json"
    local health_score=$(calculate_health_score)
    
    cat > "$report_file" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "period_hours": $LOOKBACK_HOURS,
  "health_score": $health_score,
  "api_performance": {
    "p99_latency_seconds": $P99_LATENCY,
    "p95_latency_seconds": $P95_LATENCY,
    "p50_latency_seconds": $P50_LATENCY,
    "error_rate": $ERROR_RATE,
    "total_requests": $TOTAL_REQUESTS
  },
  "memory": {
    "current_mb": $CURRENT_MEMORY_MB,
    "peak_mb": $PEAK_MEMORY_MB
  },
  "database": {
    "p99_query_time_seconds": $DB_P99_QUERY_TIME
  },
  "reconciliation": {
    "total_runs": $RECONCILIATION_RUNS,
    "failures": $RECONCILIATION_FAILURES,
    "avg_drift_stroops": $AVG_DRIFT_STROOPS
  },
  "circuit_breakers": {
    "total_trips": $CIRCUIT_BREAKER_TRIPS
  },
  "log_management": {
    "delivery_rate": $LOG_DELIVERY_RATE
  }
}
EOF
    
    echo "$report_file"
}

# ============================================================================
# Send to Slack
# ============================================================================
send_slack_notification() {
    local text_report="$1"
    local health_score=$(calculate_health_score)
    
    if [ -z "$SLACK_WEBHOOK" ]; then
        log_warn "Slack webhook not configured, skipping notification"
        return 0
    fi
    
    local color="good"
    if [ "$health_score" -lt 80 ]; then
        color="warning"
    fi
    if [ "$health_score" -lt 60 ]; then
        color="danger"
    fi
    
    local payload=$(cat <<EOF
{
  "username": "Aframp Performance Bot",
  "icon_emoji": ":chart_with_upwards_trend:",
  "attachments": [
    {
      "color": "$color",
      "title": "Daily Performance Report - $TIMESTAMP",
      "text": "Health Score: ${health_score}/100",
      "fields": [
        {
          "title": "P99 Latency",
          "value": "$(printf "%.3f" "$P99_LATENCY")s",
          "short": true
        },
        {
          "title": "Error Rate",
          "value": "$(printf "%.4f%%" "$(echo "$ERROR_RATE * 100" | bc)")",
          "short": true
        },
        {
          "title": "Total Requests",
          "value": "$(printf "%.0f" "$TOTAL_REQUESTS")",
          "short": true
        },
        {
          "title": "Reconciliation Failures",
          "value": "$(printf "%.0f" "$RECONCILIATION_FAILURES")",
          "short": true
        }
      ],
      "footer": "Aframp Production Operations",
      "ts": $(date +%s)
    }
  ]
}
EOF
)
    
    curl -X POST "$SLACK_WEBHOOK" \
        -H 'Content-Type: application/json' \
        -d "$payload" \
        > /dev/null 2>&1
    
    log_info "Slack notification sent"
}

# ============================================================================
# Main Execution
# ============================================================================
main() {
    log_info "Starting daily performance report generation"
    
    collect_metrics
    
    TEXT_REPORT=$(generate_text_report)
    log_info "Text report generated: $TEXT_REPORT"
    
    JSON_REPORT=$(generate_json_report)
    log_info "JSON report generated: $JSON_REPORT"
    
    cat "$TEXT_REPORT"
    
    send_slack_notification "$TEXT_REPORT"
    
    log_info "Daily performance report completed"
}

main "$@"
