-- ============================================================================
-- Automated Database Maintenance & Partitioning
-- ============================================================================
-- Implements:
--   1. Declarative partitioning for metrics tables
--   2. Autovacuum optimization settings
--   3. Cold storage migration infrastructure
--   4. Automated partition management functions
-- ============================================================================

-- Enable required extensions
CREATE EXTENSION IF NOT EXISTS pg_partman;
CREATE EXTENSION IF NOT EXISTS pg_cron;

-- ============================================================================
-- 1. Partitioned Tables for High-Volume Metrics
-- ============================================================================

-- Risk exposure snapshots (time-series partitioned by day)
CREATE TABLE IF NOT EXISTS risk_exposure_snapshots (
    id BIGSERIAL NOT NULL,
    snapshot_time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    partner_id UUID NOT NULL,
    currency_code VARCHAR(3) NOT NULL,
    exposure_amount DECIMAL(24, 7) NOT NULL,
    risk_score DECIMAL(5, 2),
    threshold_breached BOOLEAN DEFAULT FALSE,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
) PARTITION BY RANGE (snapshot_time);

-- Create initial partitions (last 7 days + next 7 days)
CREATE TABLE risk_exposure_snapshots_default PARTITION OF risk_exposure_snapshots DEFAULT;

DO $$
DECLARE
    partition_date DATE;
    partition_name TEXT;
    start_date DATE;
    end_date DATE;
BEGIN
    FOR i IN -7..7 LOOP
        partition_date := CURRENT_DATE + (i || ' days')::INTERVAL;
        partition_name := 'risk_exposure_snapshots_' || TO_CHAR(partition_date, 'YYYY_MM_DD');
        start_date := partition_date;
        end_date := partition_date + INTERVAL '1 day';
        
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF risk_exposure_snapshots
             FOR VALUES FROM (%L) TO (%L)',
            partition_name, start_date, end_date
        );
    END LOOP;
END $$;

CREATE INDEX idx_risk_exposure_partner ON risk_exposure_snapshots(partner_id, snapshot_time DESC);
CREATE INDEX idx_risk_exposure_currency ON risk_exposure_snapshots(currency_code, snapshot_time DESC);
CREATE INDEX idx_risk_exposure_threshold ON risk_exposure_snapshots(threshold_breached, snapshot_time DESC) WHERE threshold_breached = TRUE;

-- Partner performance logs (time-series partitioned by day)
CREATE TABLE IF NOT EXISTS partner_performance_logs (
    id BIGSERIAL NOT NULL,
    log_time TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    partner_id UUID NOT NULL,
    operation_type VARCHAR(50) NOT NULL,
    response_time_ms INTEGER,
    success BOOLEAN NOT NULL,
    error_code VARCHAR(50),
    request_payload_hash VARCHAR(64),
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
) PARTITION BY RANGE (log_time);

-- Create initial partitions
CREATE TABLE partner_performance_logs_default PARTITION OF partner_performance_logs DEFAULT;

DO $$
DECLARE
    partition_date DATE;
    partition_name TEXT;
    start_date DATE;
    end_date DATE;
BEGIN
    FOR i IN -7..7 LOOP
        partition_date := CURRENT_DATE + (i || ' days')::INTERVAL;
        partition_name := 'partner_performance_logs_' || TO_CHAR(partition_date, 'YYYY_MM_DD');
        start_date := partition_date;
        end_date := partition_date + INTERVAL '1 day';
        
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS %I PARTITION OF partner_performance_logs
             FOR VALUES FROM (%L) TO (%L)',
            partition_name, start_date, end_date
        );
    END LOOP;
END $$;

CREATE INDEX idx_partner_perf_partner ON partner_performance_logs(partner_id, log_time DESC);
CREATE INDEX idx_partner_perf_operation ON partner_performance_logs(operation_type, log_time DESC);
CREATE INDEX idx_partner_perf_errors ON partner_performance_logs(error_code, log_time DESC) WHERE error_code IS NOT NULL;

-- ============================================================================
-- 2. Automated Partition Management
-- ============================================================================

CREATE TABLE IF NOT EXISTS partition_management_log (
    id BIGSERIAL PRIMARY KEY,
    table_name VARCHAR(255) NOT NULL,
    partition_name VARCHAR(255) NOT NULL,
    action VARCHAR(50) NOT NULL, -- 'CREATE', 'DROP', 'ARCHIVE'
    partition_start TIMESTAMPTZ,
    partition_end TIMESTAMPTZ,
    execution_time_ms INTEGER,
    success BOOLEAN NOT NULL,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_partition_mgmt_table ON partition_management_log(table_name, created_at DESC);
CREATE INDEX idx_partition_mgmt_action ON partition_management_log(action, created_at DESC);

-- Function to create future partitions
CREATE OR REPLACE FUNCTION create_future_partitions(
    p_table_name TEXT,
    p_days_ahead INTEGER DEFAULT 7
) RETURNS INTEGER AS $$
DECLARE
    v_partition_date DATE;
    v_partition_name TEXT;
    v_start_date DATE;
    v_end_date DATE;
    v_partitions_created INTEGER := 0;
    v_start_time TIMESTAMPTZ;
    v_execution_time INTEGER;
BEGIN
    v_start_time := clock_timestamp();
    
    FOR i IN 1..p_days_ahead LOOP
        v_partition_date := CURRENT_DATE + (i || ' days')::INTERVAL;
        v_partition_name := p_table_name || '_' || TO_CHAR(v_partition_date, 'YYYY_MM_DD');
        v_start_date := v_partition_date;
        v_end_date := v_partition_date + INTERVAL '1 day';
        
        -- Check if partition already exists
        IF NOT EXISTS (
            SELECT 1 FROM pg_tables 
            WHERE tablename = v_partition_name
        ) THEN
            BEGIN
                EXECUTE format(
                    'CREATE TABLE %I PARTITION OF %I
                     FOR VALUES FROM (%L) TO (%L)',
                    v_partition_name, p_table_name, v_start_date, v_end_date
                );
                
                v_partitions_created := v_partitions_created + 1;
                
                v_execution_time := EXTRACT(MILLISECONDS FROM clock_timestamp() - v_start_time)::INTEGER;
                
                INSERT INTO partition_management_log (
                    table_name, partition_name, action,
                    partition_start, partition_end,
                    execution_time_ms, success
                ) VALUES (
                    p_table_name, v_partition_name, 'CREATE',
                    v_start_date, v_end_date,
                    v_execution_time, TRUE
                );
                
            EXCEPTION WHEN OTHERS THEN
                INSERT INTO partition_management_log (
                    table_name, partition_name, action,
                    partition_start, partition_end,
                    execution_time_ms, success, error_message
                ) VALUES (
                    p_table_name, v_partition_name, 'CREATE',
                    v_start_date, v_end_date,
                    v_execution_time, FALSE, SQLERRM
                );
            END;
        END IF;
    END LOOP;
    
    RETURN v_partitions_created;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- 3. Cold Storage Migration Infrastructure
-- ============================================================================

CREATE TABLE IF NOT EXISTS cold_storage_archive_log (
    id BIGSERIAL PRIMARY KEY,
    source_table VARCHAR(255) NOT NULL,
    partition_name VARCHAR(255) NOT NULL,
    records_archived BIGINT NOT NULL,
    storage_location TEXT NOT NULL, -- S3 path or equivalent
    archive_format VARCHAR(50) NOT NULL, -- 'parquet', 'csv.gz', etc.
    integrity_hash VARCHAR(64) NOT NULL, -- SHA-256
    compression_ratio DECIMAL(5, 2),
    archive_size_bytes BIGINT,
    archived_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB
);

CREATE INDEX idx_cold_storage_table ON cold_storage_archive_log(source_table, archived_at DESC);
CREATE INDEX idx_cold_storage_partition ON cold_storage_archive_log(partition_name);

-- Function to identify partitions eligible for archival (older than 90 days)
CREATE OR REPLACE FUNCTION get_archival_candidates(
    p_table_name TEXT,
    p_retention_days INTEGER DEFAULT 90
) RETURNS TABLE (
    partition_name TEXT,
    partition_start TIMESTAMPTZ,
    partition_end TIMESTAMPTZ,
    estimated_rows BIGINT
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        c.relname::TEXT,
        pg_catalog.pg_get_expr(c.relpartbound, c.oid)::TEXT AS bounds,
        NULL::TIMESTAMPTZ AS end_date,
        c.reltuples::BIGINT
    FROM pg_catalog.pg_class c
    JOIN pg_catalog.pg_inherits i ON c.oid = i.inhrelid
    JOIN pg_catalog.pg_class p ON i.inhparent = p.oid
    WHERE p.relname = p_table_name
    AND c.relname != p_table_name || '_default'
    AND c.relname ~ '_\d{4}_\d{2}_\d{2}$'
    AND TO_DATE(
        regexp_replace(c.relname, '^.*_(\d{4}_\d{2}_\d{2})$', '\1'),
        'YYYY_MM_DD'
    ) < CURRENT_DATE - p_retention_days;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- 4. Optimized Autovacuum Settings
-- ============================================================================

-- Configure aggressive autovacuum for high-throughput tables
ALTER TABLE transactions SET (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_vacuum_cost_delay = 2,
    autovacuum_vacuum_cost_limit = 1000
);

ALTER TABLE payment_ledger SET (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_vacuum_cost_delay = 2,
    autovacuum_vacuum_cost_limit = 1000
);

-- For append-only audit tables, reduce vacuum frequency but increase efficiency
ALTER TABLE audit_log_append_only SET (
    autovacuum_vacuum_scale_factor = 0.1,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_freeze_max_age = 200000000
);

-- ============================================================================
-- 5. Index Maintenance Scheduler
-- ============================================================================

CREATE TABLE IF NOT EXISTS index_maintenance_log (
    id BIGSERIAL PRIMARY KEY,
    table_name VARCHAR(255) NOT NULL,
    index_name VARCHAR(255) NOT NULL,
    operation VARCHAR(50) NOT NULL, -- 'REINDEX', 'ANALYZE'
    bloat_ratio DECIMAL(5, 2),
    execution_time_ms INTEGER,
    success BOOLEAN NOT NULL,
    error_message TEXT,
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_index_maint_table ON index_maintenance_log(table_name, executed_at DESC);

-- Function to reindex bloated indexes
CREATE OR REPLACE FUNCTION reindex_bloated_indexes(
    p_bloat_threshold DECIMAL DEFAULT 30.0
) RETURNS INTEGER AS $$
DECLARE
    v_rec RECORD;
    v_reindexed INTEGER := 0;
    v_start_time TIMESTAMPTZ;
    v_execution_time INTEGER;
BEGIN
    -- Identify bloated indexes (simplified query)
    FOR v_rec IN
        SELECT 
            schemaname,
            tablename,
            indexname
        FROM pg_indexes
        WHERE schemaname = 'public'
        AND indexname NOT LIKE 'pg_%'
    LOOP
        BEGIN
            v_start_time := clock_timestamp();
            
            -- Perform REINDEX CONCURRENTLY to avoid blocking
            EXECUTE format('REINDEX INDEX CONCURRENTLY %I', v_rec.indexname);
            
            v_execution_time := EXTRACT(MILLISECONDS FROM clock_timestamp() - v_start_time)::INTEGER;
            v_reindexed := v_reindexed + 1;
            
            INSERT INTO index_maintenance_log (
                table_name, index_name, operation,
                execution_time_ms, success
            ) VALUES (
                v_rec.tablename, v_rec.indexname, 'REINDEX',
                v_execution_time, TRUE
            );
            
        EXCEPTION WHEN OTHERS THEN
            INSERT INTO index_maintenance_log (
                table_name, index_name, operation,
                execution_time_ms, success, error_message
            ) VALUES (
                v_rec.tablename, v_rec.indexname, 'REINDEX',
                0, FALSE, SQLERRM
            );
        END;
    END LOOP;
    
    RETURN v_reindexed;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- 6. Scheduled Jobs (using pg_cron if available)
-- ============================================================================

-- Create partitions daily at 2 AM
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_cron') THEN
        PERFORM cron.schedule(
            'create-risk-partitions',
            '0 2 * * *',
            $$SELECT create_future_partitions('risk_exposure_snapshots', 7)$$
        );
        
        PERFORM cron.schedule(
            'create-performance-partitions',
            '0 2 * * *',
            $$SELECT create_future_partitions('partner_performance_logs', 7)$$
        );
        
        -- Weekly index maintenance on Sundays at 3 AM
        PERFORM cron.schedule(
            'weekly-index-maintenance',
            '0 3 * * 0',
            $$SELECT reindex_bloated_indexes(30.0)$$
        );
    END IF;
END $$;

-- ============================================================================
-- 7. Monitoring Views
-- ============================================================================

CREATE OR REPLACE VIEW v_partition_health AS
SELECT 
    schemaname,
    tablename AS partition_name,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS total_size,
    pg_stat_get_live_tuples(schemaname||'.'||tablename::regclass) AS live_rows,
    pg_stat_get_dead_tuples(schemaname||'.'||tablename::regclass) AS dead_rows,
    ROUND(
        100.0 * pg_stat_get_dead_tuples(schemaname||'.'||tablename::regclass) / 
        NULLIF(pg_stat_get_live_tuples(schemaname||'.'||tablename::regclass), 0),
        2
    ) AS dead_row_percentage
FROM pg_tables
WHERE schemaname = 'public'
AND (tablename LIKE 'risk_exposure_snapshots_%' OR tablename LIKE 'partner_performance_logs_%')
ORDER BY tablename DESC;

CREATE OR REPLACE VIEW v_autovacuum_activity AS
SELECT 
    schemaname,
    relname,
    last_vacuum,
    last_autovacuum,
    last_analyze,
    last_autoanalyze,
    vacuum_count,
    autovacuum_count,
    analyze_count,
    autoanalyze_count
FROM pg_stat_user_tables
WHERE schemaname = 'public'
ORDER BY last_autovacuum DESC NULLS LAST;

COMMENT ON TABLE risk_exposure_snapshots IS 'Partitioned metrics table for partner risk exposure tracking';
COMMENT ON TABLE partner_performance_logs IS 'Partitioned logs for partner API performance monitoring';
COMMENT ON TABLE partition_management_log IS 'Audit log for automated partition lifecycle operations';
COMMENT ON TABLE cold_storage_archive_log IS 'Registry of archived partitions with integrity proofs';
