# Alerting Integration Tests - Panic-Prone Calls Fix

## Summary
Fixed all 15 panic-prone `unwrap()` calls in `tests/alerting_integration.rs` by replacing them with `expect()` calls that include descriptive error messages explaining the failure context.

## Changes Made

### File: `tests/alerting_integration.rs`

#### Metric Registration Functions (13 instances)
Replaced `unwrap()` with `expect()` including context-specific error messages in:
- `make_http_requests_total()` - Line 34
- `make_cngn_transactions_total()` - Line 44
- `make_stellar_submissions_total()` - Line 54
- `make_worker_errors_total()` - Line 64
- `make_worker_cycles_total()` - Line 74
- `make_db_errors_total()` - Line 84
- `make_payment_provider_failures_total()` - Line 94
- `make_exchange_rate_last_updated()` - Line 104
- `make_worker_last_cycle_timestamp()` - Line 114
- `make_pending_transactions_stale()` - Line 124
- `make_rate_limit_breaches_total()` - Line 134
- `make_cache_hits_total()` - Line 144
- `make_cache_misses_total()` - Line 154

**Error message pattern**: "Failed to register {metric_name} metric - this is a test setup error indicating registry conflict"

#### Render Function (2 instances)
Replaced `unwrap()` with `expect()` in the `render()` function:
1. **Encoder encoding** - Line 162: "Failed to encode Prometheus metrics - this indicates a serialization error in the test"
2. **UTF-8 conversion** - Line 164: "Failed to convert Prometheus metrics to UTF-8 - this indicates corrupt metric data in the test"

## Rationale

### Why `expect()` instead of `Result` propagation?
These are test helper functions that establish invariants required for tests to run. Failures here indicate:
- Registry conflicts (metric already registered)
- Serialization errors (corrupt internal state)
- UTF-8 conversion errors (corrupt metric data)

All of these are **unrecoverable test setup errors** that should halt execution immediately with clear diagnostic information.

### Documented Invariants
Each `expect()` call includes a descriptive message that:
1. Identifies what failed
2. Explains why it failed (root cause category)
3. Helps developers diagnose the issue quickly

## Acceptance Criteria ✓

- [x] All 15 avoidable `unwrap()` calls removed
- [x] Each `expect()` includes justified, descriptive error context
- [x] Error messages preserve observability context
- [x] No diagnostics errors found in the file
- [x] Changes maintain test isolation (each test uses independent registries)

## Testing

File passes static analysis with no diagnostics errors. The use of `expect()` is justified because:
1. These are test-only helpers, not production code
2. Failures indicate test setup issues, not runtime errors
3. Each panic is well-documented with clear error messages
4. Tests cannot meaningfully continue if metric registration fails

## Notes

- No production code affected (test file only)
- Test structure unchanged - still using isolated registries
- All error messages follow consistent format
- Zero remaining `unwrap()` calls in the file
