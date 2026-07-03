#!/usr/bin/env bash
# ============================================================================
# Automated Log Rotation and Archival Script
# ============================================================================
# Daily log rotation with PII masking, compression, and cold storage archival
# ============================================================================

set -euo pipefail

# Configuration
LOG_DIR="${LOG_DIR:-/var/log/aframp}"
ARCHIVE_DIR="${ARCHIVE_DIR:-/var/log/aframp/archive}"
RETENTION_DAYS="${RETENTION_DAYS:-90}"
COMPRESSION_LEVEL="${COMPRESSION_LEVEL:-9}"
S3_BUCKET="${S3_BUCKET:-s3://aframp-logs-archive}"
ENABLE_S3_UPLOAD="${ENABLE_S3_UPLOAD:-true}"
ENABLE_ENCRYPTION="${ENABLE_ENCRYPTION:-true}"
KMS_KEY_ID="${KMS_KEY_ID:-alias/aframp-logs}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*" >&2
}

# Create archive directory if it doesn't exist
mkdir -p "$ARCHIVE_DIR"

# ============================================================================
# PII Masking Function
# ============================================================================
mask_pii() {
    local input_file="$1"
    local output_file="$2"
    
    log_info "Masking PII in $input_file"
    
    # Use sed for efficient stream processing
    sed -E \
        -e 's/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/***@***.com/g' \
        -e 's/\+?[0-9]{10,15}/***-***-****/g' \
        -e 's/\b[0-9]{4}[-\s]?[0-9]{4}[-\s]?[0-9]{4}[-\s]?[0-9]{4}\b/****-****-****-****/g' \
        -e 's/(api[_-]?key|token|secret)["\s:=]+[a-zA-Z0-9\-_]{20,}/\1=***REDACTED***/gi' \
        "$input_file" > "$output_file"
    
    if [ $? -eq 0 ]; then
        log_info "PII masking completed successfully"
        return 0
    else
        log_error "PII masking failed"
        return 1
    fi
}

# ============================================================================
# Compression Function
# ============================================================================
compress_log() {
    local input_file="$1"
    local output_file="$2"
    
    log_info "Compressing $input_file with gzip -$COMPRESSION_LEVEL"
    
    gzip "-$COMPRESSION_LEVEL" -c "$input_file" > "$output_file"
    
    if [ $? -eq 0 ]; then
        local original_size=$(stat -f%z "$input_file" 2>/dev/null || stat -c%s "$input_file")
        local compressed_size=$(stat -f%z "$output_file" 2>/dev/null || stat -c%s "$output_file")
        local ratio=$(awk "BEGIN {printf \"%.2f\", ($original_size - $compressed_size) / $original_size * 100}")
        
        log_info "Compression complete: ${ratio}% reduction (${original_size} -> ${compressed_size} bytes)"
        return 0
    else
        log_error "Compression failed"
        return 1
    fi
}

# ============================================================================
# Encryption Function
# ============================================================================
encrypt_file() {
    local input_file="$1"
    local output_file="$2"
    
    if [ "$ENABLE_ENCRYPTION" != "true" ]; then
        log_info "Encryption disabled, copying file"
        cp "$input_file" "$output_file"
        return 0
    fi
    
    log_info "Encrypting $input_file with AES-256-GCM"
    
    # Generate encryption key from KMS
    if command -v aws &> /dev/null; then
        openssl enc -aes-256-gcm -salt -pbkdf2 -in "$input_file" -out "$output_file"
        
        if [ $? -eq 0 ]; then
            log_info "Encryption completed successfully"
            return 0
        else
            log_error "Encryption failed"
            return 1
        fi
    else
        log_warn "AWS CLI not available, skipping encryption"
        cp "$input_file" "$output_file"
        return 0
    fi
}

# ============================================================================
# S3 Upload Function
# ============================================================================
upload_to_s3() {
    local local_file="$1"
    local s3_path="$2"
    
    if [ "$ENABLE_S3_UPLOAD" != "true" ]; then
        log_info "S3 upload disabled"
        return 0
    fi
    
    if ! command -v aws &> /dev/null; then
        log_warn "AWS CLI not available, skipping S3 upload"
        return 0
    fi
    
    log_info "Uploading $local_file to $s3_path"
    
    # Calculate SHA-256 integrity hash
    local sha256sum
    if command -v sha256sum &> /dev/null; then
        sha256sum=$(sha256sum "$local_file" | awk '{print $1}')
    elif command -v shasum &> /dev/null; then
        sha256sum=$(shasum -a 256 "$local_file" | awk '{print $1}')
    else
        log_warn "SHA-256 calculation not available"
        sha256sum="unavailable"
    fi
    
    # Upload with metadata
    aws s3 cp "$local_file" "$s3_path" \
        --metadata "sha256=$sha256sum,rotation-date=$(date -I)" \
        --storage-class INTELLIGENT_TIERING \
        --server-side-encryption aws:kms \
        --ssekms-key-id "$KMS_KEY_ID"
    
    if [ $? -eq 0 ]; then
        log_info "S3 upload completed successfully (SHA-256: $sha256sum)"
        return 0
    else
        log_error "S3 upload failed"
        return 1
    fi
}

# ============================================================================
# Main Rotation Logic
# ============================================================================
rotate_logs() {
    local date_suffix=$(date -d "yesterday" '+%Y-%m-%d' 2>/dev/null || date -v-1d '+%Y-%m-%d')
    
    log_info "Starting log rotation for date: $date_suffix"
    
    # Find all log files to rotate (not already rotated)
    find "$LOG_DIR" -maxdepth 1 -type f -name "*.log" ! -name "*.gz" | while read -r logfile; do
        local basename=$(basename "$logfile" .log)
        local temp_masked="${ARCHIVE_DIR}/${basename}_${date_suffix}_masked.log"
        local temp_compressed="${ARCHIVE_DIR}/${basename}_${date_suffix}.log.gz"
        local final_encrypted="${ARCHIVE_DIR}/${basename}_${date_suffix}.log.gz.enc"
        
        log_info "Processing $logfile"
        
        # Step 1: Mask PII
        if mask_pii "$logfile" "$temp_masked"; then
            
            # Step 2: Compress
            if compress_log "$temp_masked" "$temp_compressed"; then
                rm "$temp_masked"  # Remove intermediate file
                
                # Step 3: Encrypt
                if encrypt_file "$temp_compressed" "$final_encrypted"; then
                    rm "$temp_compressed"  # Remove intermediate file
                    
                    # Step 4: Upload to S3
                    local s3_date_path=$(date -d "yesterday" '+%Y/%m/%d' 2>/dev/null || date -v-1d '+%Y/%m/%d')
                    local s3_path="${S3_BUCKET}/${s3_date_path}/$(basename "$final_encrypted")"
                    
                    if upload_to_s3 "$final_encrypted" "$s3_path"; then
                        log_info "Successfully archived $logfile"
                        
                        # Truncate original log file
                        > "$logfile"
                        log_info "Truncated $logfile"
                    else
                        log_error "Failed to upload $logfile to S3"
                    fi
                else
                    log_error "Failed to encrypt $logfile"
                fi
            else
                log_error "Failed to compress $logfile"
            fi
        else
            log_error "Failed to mask PII in $logfile"
        fi
    done
}

# ============================================================================
# Cleanup Old Archives
# ============================================================================
cleanup_old_archives() {
    log_info "Cleaning up archives older than $RETENTION_DAYS days"
    
    find "$ARCHIVE_DIR" -type f -mtime "+$RETENTION_DAYS" -exec rm -f {} \;
    
    log_info "Cleanup completed"
}

# ============================================================================
# Generate Rotation Report
# ============================================================================
generate_report() {
    local report_file="${ARCHIVE_DIR}/rotation_report_$(date '+%Y-%m-%d').txt"
    
    {
        echo "Log Rotation Report - $(date)"
        echo "================================"
        echo ""
        echo "Archive Directory: $ARCHIVE_DIR"
        echo "Total Files: $(find "$ARCHIVE_DIR" -type f | wc -l)"
        echo "Total Size: $(du -sh "$ARCHIVE_DIR" | awk '{print $1}')"
        echo ""
        echo "Files by Type:"
        echo "  Encrypted: $(find "$ARCHIVE_DIR" -name "*.enc" | wc -l)"
        echo "  Compressed: $(find "$ARCHIVE_DIR" -name "*.gz" ! -name "*.enc" | wc -l)"
        echo ""
        echo "Oldest Archive: $(find "$ARCHIVE_DIR" -type f -printf '%T+ %p\n' 2>/dev/null | sort | head -n1 || echo 'N/A')"
        echo "Newest Archive: $(find "$ARCHIVE_DIR" -type f -printf '%T+ %p\n' 2>/dev/null | sort | tail -n1 || echo 'N/A')"
    } > "$report_file"
    
    log_info "Report generated: $report_file"
}

# ============================================================================
# Main Execution
# ============================================================================
main() {
    log_info "Starting automated log rotation"
    
    rotate_logs
    cleanup_old_archives
    generate_report
    
    log_info "Log rotation completed successfully"
}

# Run main function
main "$@"
