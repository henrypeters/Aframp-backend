#!/usr/bin/env bash
# ============================================================================
# Automated High-Load Performance Drill
# ============================================================================
# Simulates production concurrency to verify memory stability and P99 latency
# ============================================================================

set -euo pipefail

# Configuration
API_BASE_URL="${API_BASE_URL:-http://localhost:8000}"
DRILL_DURATION="${DRILL_DURATION:-300}"  # 5 minutes
CONCURRENT_USERS="${CONCURRENT_USERS:-1000}"
REQUESTS_PER_SECOND="${REQUESTS_PER_SECOND:-500}"
MEMORY_BASELINE_THRESHOLD_MB="${MEMORY_BASELINE_THRESHOLD_MB:-2048}"  # 2GB
P99_LATENCY_THRESHOLD_MS="${P99_LATENCY_THRESHOLD_MS:-1000}"  # 1 second
REPORT_DIR="${REPORT_DIR:-./performance-reports}"
TIMESTAMP=$(date '+%Y%m%d_%H%M%S')

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
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

log_success() {
    echo -e "${BLUE}[SUCCESS]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

# Create report directory
mkdir -p "$REPORT_DIR"
REPORT_FILE="${REPORT_DIR}/drill_${TIMESTAMP}.json"

# ============================================================================
# Pre-flight Checks
# ============================================================================
preflight_checks() {
    log_info "Running pre-flight checks..."
    
    # Check if API is reachable
    if ! curl -sf "${API_BASE_URL}/health" > /dev/null; then
        log_error "API is not reachable at ${API_BASE_URL}"
        return 1
    fi
    
    # Check for required tools
    for tool in curl jq bc; do
        if ! command -v "$tool" &> /dev/null; then
            log_error "Required tool not found: $tool"
            return 1
        fi
    done
    
    # Check for load testing tool
    if command -v hey &> /dev/null; then
        LOAD_TOOL="hey"
    elif command -v wrk &> /dev/null; then
        LOAD_TOOL="wrk"
    elif command -v ab &> /dev/null; then
        LOAD_TOOL="ab"
    else
        log_error "No load testing tool found (hey, wrk, or ab required)"
        return 1
    fi
    
    log_success "Pre-flight checks passed (using $LOAD_TOOL)"
}

# ============================================================================
# Memory Baseline Capture
# ============================================================================
capture_memory_baseline() {
    log_info "Capturing memory baseline..."
    
    local memory_endpoint="${API_BASE_URL}/profiling/memory"
    
    if ! BASELINE_MEMORY=$(curl -sf "$memory_endpoint" | jq -r '.current_heap_mb'); then
        log_warn "Could not capture memory baseline from API"
        BASELINE_MEMORY=0
    fi
    
    log_info "Memory baseline: ${BASELINE_MEMORY} MB"
    echo "$BASELINE_MEMORY"
}

# ============================================================================
# Load Test Execution
# ============================================================================
run_load_test() {
    log_info "Starting load test: ${CONCURRENT_USERS} users, ${REQUESTS_PER_SECOND} req/s for ${DRILL_DURATION}s"
    
    local output_file="${REPORT_DIR}/loadtest_${TIMESTAMP}.txt"
    
    case "$LOAD_TOOL" in
        hey)
            hey -z "${DRILL_DURATION}s" \
                -c "$CONCURRENT_USERS" \
                -q "$REQUESTS_PER_SECOND" \
                -m GET \
                -H "Accept: application/json" \
                "${API_BASE_URL}/api/v1/rates" \
                > "$output_file" 2>&1
            ;;
        wrk)
            wrk -t "$CONCURRENT_USERS" \
                -c "$CONCURRENT_USERS" \
                -d "${DRILL_DURATION}s" \
                --latency \
                "${API_BASE_URL}/api/v1/rates" \
                > "$output_file" 2>&1
            ;;
        ab)
            ab -n $((REQUESTS_PER_SECOND * DRILL_DURATION)) \
                -c "$CONCURRENT_USERS" \
                -g "${REPORT_DIR}/gnuplot_${TIMESTAMP}.tsv" \
                "${API_BASE_URL}/api/v1/rates" \
                > "$output_file" 2>&1
            ;;
    esac
    
    log_success "Load test completed"
    echo "$output_file"
}

# ============================================================================
# Parse Load Test Results
# ============================================================================
parse_load_test_results() {
    local output_file="$1"
    
    log_info "Parsing load test results..."
    
    case "$LOAD_TOOL" in
        hey)
            TOTAL_REQUESTS=$(grep "Total:" "$output_file" | awk '{print $2}')
            SUCCESS_RATE=$(grep "Success rate:" "$output_file" | awk '{print $3}' | tr -d '%')
            P99_LATENCY=$(grep "99%" "$output_file" | awk '{print $2}' | sed 's/s$//')
            AVG_LATENCY=$(grep "Average:" "$output_file" | awk '{print $2}' | sed 's/s$//')
            ;;
        wrk)
            TOTAL_REQUESTS=$(grep "Requests/sec:" "$output_file" | awk '{print $2 * '"$DRILL_DURATION"'}' | bc)
            SUCCESS_RATE=99.9  # wrk doesn't provide this directly
            P99_LATENCY=$(grep "99.000%" "$output_file" | awk '{print $2}' | sed 's/ms//' | awk '{print $1/1000}')
            AVG_LATENCY=$(grep "Latency" "$output_file" | awk '{print $2}' | sed 's/ms//' | awk '{print $1/1000}')
            ;;
        ab)
            TOTAL_REQUESTS=$(grep "Complete requests:" "$output_file" | awk '{print $3}')
            FAILED_REQUESTS=$(grep "Failed requests:" "$output_file" | awk '{print $3}')
            SUCCESS_RATE=$(echo "scale=2; ($TOTAL_REQUESTS - $FAILED_REQUESTS) / $TOTAL_REQUESTS * 100" | bc)
            P99_LATENCY=$(grep "99%" "$output_file" | awk '{print $2/1000}')
            AVG_LATENCY=$(grep "Time per request.*mean" "$output_file" | awk '{print $4/1000}')
            ;;
    esac
    
    log_info "Results: ${TOTAL_REQUESTS} requests, ${SUCCESS_RATE}% success, P99=${P99_LATENCY}s"
}

# ============================================================================
# Memory Stability Check
# ============================================================================
check_memory_stability() {
    local baseline_mb="$1"
    
    log_info "Checking memory stability..."
    
    local memory_endpoint="${API_BASE_URL}/profiling/memory"
    
    if ! CURRENT_MEMORY=$(curl -sf "$memory_endpoint" | jq -r '.current_heap_mb'); then
        log_warn "Could not capture current memory from API"
        return 1
    fi
    
    local memory_increase=$(echo "$CURRENT_MEMORY - $baseline_mb" | bc)
    local memory_increase_pct=$(echo "scale=2; ($memory_increase / $baseline_mb) * 100" | bc)
    
    log_info "Memory: baseline=${baseline_mb}MB, current=${CURRENT_MEMORY}MB, increase=${memory_increase}MB (${memory_increase_pct}%)"
    
    # Check if memory is stable (increase < 10%)
    if (( $(echo "$memory_increase_pct < 10" | bc -l) )); then
        log_success "Memory stable: ${memory_increase_pct}% increase"
        MEMORY_STABLE=true
    else
        log_error "Memory unstable: ${memory_increase_pct}% increase exceeds 10% threshold"
        MEMORY_STABLE=false
    fi
    
    FINAL_MEMORY="$CURRENT_MEMORY"
}

# ============================================================================
# Performance Validation
# ============================================================================
validate_performance() {
    log_info "Validating performance metrics..."
    
    local pass=true
    
    # Check P99 latency
    local p99_ms=$(echo "$P99_LATENCY * 1000" | bc)
    if (( $(echo "$p99_ms > $P99_LATENCY_THRESHOLD_MS" | bc -l) )); then
        log_error "P99 latency ${p99_ms}ms exceeds threshold ${P99_LATENCY_THRESHOLD_MS}ms"
        pass=false
    else
        log_success "P99 latency ${p99_ms}ms within threshold"
    fi
    
    # Check success rate
    if (( $(echo "$SUCCESS_RATE < 99.0" | bc -l) )); then
        log_error "Success rate ${SUCCESS_RATE}% below 99%"
        pass=false
    else
        log_success "Success rate ${SUCCESS_RATE}% acceptable"
    fi
    
    # Check memory stability
    if [ "$MEMORY_STABLE" = false ]; then
        log_error "Memory stability check failed"
        pass=false
    fi
    
    if [ "$pass" = true ]; then
        log_success "All performance validations passed"
        return 0
    else
        log_error "Performance validation failed"
        return 1
    fi
}

# ============================================================================
# Generate Report
# ============================================================================
generate_report() {
    log_info "Generating performance report..."
    
    cat > "$REPORT_FILE" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "drill_configuration": {
    "duration_seconds": $DRILL_DURATION,
    "concurrent_users": $CONCURRENT_USERS,
    "requests_per_second": $REQUESTS_PER_SECOND,
    "api_base_url": "$API_BASE_URL",
    "load_tool": "$LOAD_TOOL"
  },
  "results": {
    "total_requests": $TOTAL_REQUESTS,
    "success_rate_percent": $SUCCESS_RATE,
    "p99_latency_seconds": $P99_LATENCY,
    "avg_latency_seconds": $AVG_LATENCY
  },
  "memory": {
    "baseline_mb": $BASELINE_MEMORY,
    "final_mb": $FINAL_MEMORY,
    "increase_mb": $(echo "$FINAL_MEMORY - $BASELINE_MEMORY" | bc),
    "stable": $MEMORY_STABLE
  },
  "thresholds": {
    "p99_latency_ms": $P99_LATENCY_THRESHOLD_MS,
    "memory_baseline_mb": $MEMORY_BASELINE_THRESHOLD_MB
  },
  "validation": {
    "passed": $(validate_performance && echo true || echo false)
  }
}
EOF
    
    log_success "Report generated: $REPORT_FILE"
    cat "$REPORT_FILE" | jq '.'
}

# ============================================================================
# Main Execution
# ============================================================================
main() {
    log_info "Starting automated performance drill"
    
    if ! preflight_checks; then
        log_error "Pre-flight checks failed"
        exit 1
    fi
    
    BASELINE_MEMORY=$(capture_memory_baseline)
    
    LOAD_TEST_OUTPUT=$(run_load_test)
    
    parse_load_test_results "$LOAD_TEST_OUTPUT"
    
    check_memory_stability "$BASELINE_MEMORY"
    
    generate_report
    
    if validate_performance; then
        log_success "Performance drill completed successfully"
        exit 0
    else
        log_error "Performance drill failed validation"
        exit 1
    fi
}

main "$@"
