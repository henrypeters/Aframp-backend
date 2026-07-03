// ============================================================================
// Performance Profiling Infrastructure
// ============================================================================
// Implements continuous memory and CPU profiling for production workloads
// using Pyroscope integration and custom memory tracking.
// ============================================================================

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ============================================================================
// Memory Tracking
// ============================================================================

/// Global memory allocation tracker
#[derive(Debug, Clone)]
pub struct MemoryTracker {
    /// Current heap allocation in bytes
    pub current_heap_bytes: Arc<AtomicU64>,
    /// Peak heap allocation in bytes
    pub peak_heap_bytes: Arc<AtomicU64>,
    /// Total allocations count
    pub total_allocations: Arc<AtomicU64>,
    /// Total deallocations count
    pub total_deallocations: Arc<AtomicU64>,
}

impl Default for MemoryTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryTracker {
    pub fn new() -> Self {
        Self {
            current_heap_bytes: Arc::new(AtomicU64::new(0)),
            peak_heap_bytes: Arc::new(AtomicU64::new(0)),
            total_allocations: Arc::new(AtomicU64::new(0)),
            total_deallocations: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Record an allocation
    pub fn record_allocation(&self, size: u64) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        let current = self.current_heap_bytes.fetch_add(size, Ordering::SeqCst);
        let new_current = current + size;
        
        // Update peak if necessary
        let mut peak = self.peak_heap_bytes.load(Ordering::SeqCst);
        while new_current > peak {
            match self.peak_heap_bytes.compare_exchange_weak(
                peak,
                new_current,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(x) => peak = x,
            }
        }
    }

    /// Record a deallocation
    pub fn record_deallocation(&self, size: u64) {
        self.total_deallocations.fetch_add(1, Ordering::Relaxed);
        self.current_heap_bytes.fetch_sub(size, Ordering::SeqCst);
    }

    /// Get current memory statistics
    pub fn get_stats(&self) -> MemoryStats {
        MemoryStats {
            current_heap_mb: self.current_heap_bytes.load(Ordering::SeqCst) as f64 / 1_048_576.0,
            peak_heap_mb: self.peak_heap_bytes.load(Ordering::SeqCst) as f64 / 1_048_576.0,
            total_allocations: self.total_allocations.load(Ordering::Relaxed),
            total_deallocations: self.total_deallocations.load(Ordering::Relaxed),
            active_allocations: self.total_allocations.load(Ordering::Relaxed)
                - self.total_deallocations.load(Ordering::Relaxed),
        }
    }

    /// Reset peak memory tracking
    pub fn reset_peak(&self) {
        let current = self.current_heap_bytes.load(Ordering::SeqCst);
        self.peak_heap_bytes.store(current, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub current_heap_mb: f64,
    pub peak_heap_mb: f64,
    pub total_allocations: u64,
    pub total_deallocations: u64,
    pub active_allocations: u64,
}

// ============================================================================
// Hot-spot Detection
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationHotSpot {
    pub function_name: String,
    pub file_location: String,
    pub allocation_count: u64,
    pub total_bytes: u64,
    pub average_bytes: f64,
}

#[derive(Debug, Default)]
pub struct HotSpotTracker {
    hot_spots: Arc<RwLock<Vec<AllocationHotSpot>>>,
}

impl HotSpotTracker {
    pub fn new() -> Self {
        Self {
            hot_spots: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record an allocation hot-spot
    pub async fn record_hotspot(&self, function: &str, file: &str, bytes: u64) {
        let mut spots = self.hot_spots.write().await;
        
        if let Some(spot) = spots.iter_mut().find(|s| s.function_name == function) {
            spot.allocation_count += 1;
            spot.total_bytes += bytes;
            spot.average_bytes = spot.total_bytes as f64 / spot.allocation_count as f64;
        } else {
            spots.push(AllocationHotSpot {
                function_name: function.to_string(),
                file_location: file.to_string(),
                allocation_count: 1,
                total_bytes: bytes,
                average_bytes: bytes as f64,
            });
        }
    }

    /// Get top N allocation hot-spots
    pub async fn get_top_hotspots(&self, n: usize) -> Vec<AllocationHotSpot> {
        let mut spots = self.hot_spots.read().await.clone();
        spots.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));
        spots.into_iter().take(n).collect()
    }
}

// ============================================================================
// Profiling State
// ============================================================================

#[derive(Clone)]
pub struct ProfilingState {
    pub memory_tracker: MemoryTracker,
    pub hotspot_tracker: HotSpotTracker,
    pub profiling_enabled: Arc<AtomicU64>, // Using as boolean (0 or 1)
}

impl ProfilingState {
    pub fn new() -> Self {
        Self {
            memory_tracker: MemoryTracker::new(),
            hotspot_tracker: HotSpotTracker::new(),
            profiling_enabled: Arc::new(AtomicU64::new(1)), // Enabled by default
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.profiling_enabled.load(Ordering::Relaxed) == 1
    }

    pub fn enable(&self) {
        self.profiling_enabled.store(1, Ordering::Relaxed);
        info!("Profiling enabled");
    }

    pub fn disable(&self) {
        self.profiling_enabled.store(0, Ordering::Relaxed);
        warn!("Profiling disabled");
    }
}

impl Default for ProfilingState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// API Endpoints
// ============================================================================

#[derive(Serialize)]
struct ProfilingResponse {
    memory_stats: MemoryStats,
    top_hotspots: Vec<AllocationHotSpot>,
    profiling_enabled: bool,
}

/// GET /profiling/memory - Get current memory statistics
async fn get_memory_stats(
    State(state): State<ProfilingState>,
) -> Result<Json<MemoryStats>, StatusCode> {
    Ok(Json(state.memory_tracker.get_stats()))
}

/// GET /profiling/hotspots - Get allocation hot-spots
async fn get_hotspots(
    State(state): State<ProfilingState>,
) -> Result<Json<Vec<AllocationHotSpot>>, StatusCode> {
    let hotspots = state.hotspot_tracker.get_top_hotspots(20).await;
    Ok(Json(hotspots))
}

/// GET /profiling/status - Get overall profiling status
async fn get_profiling_status(
    State(state): State<ProfilingState>,
) -> Result<Json<ProfilingResponse>, StatusCode> {
    let memory_stats = state.memory_tracker.get_stats();
    let top_hotspots = state.hotspot_tracker.get_top_hotspots(10).await;
    
    Ok(Json(ProfilingResponse {
        memory_stats,
        top_hotspots,
        profiling_enabled: state.is_enabled(),
    }))
}

#[derive(Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

/// POST /profiling/toggle - Enable/disable profiling
async fn toggle_profiling(
    State(state): State<ProfilingState>,
    Json(payload): Json<ToggleRequest>,
) -> Result<StatusCode, StatusCode> {
    if payload.enabled {
        state.enable();
    } else {
        state.disable();
    }
    Ok(StatusCode::OK)
}

/// POST /profiling/reset - Reset peak memory tracking
async fn reset_peak_memory(
    State(state): State<ProfilingState>,
) -> Result<StatusCode, StatusCode> {
    state.memory_tracker.reset_peak();
    info!("Peak memory tracking reset");
    Ok(StatusCode::OK)
}

// ============================================================================
// Router Configuration
// ============================================================================

pub fn profiling_router(state: ProfilingState) -> Router {
    Router::new()
        .route("/memory", get(get_memory_stats))
        .route("/hotspots", get(get_hotspots))
        .route("/status", get(get_profiling_status))
        .route("/toggle", axum::routing::post(toggle_profiling))
        .route("/reset", axum::routing::post(reset_peak_memory))
        .with_state(state)
}

// ============================================================================
// System Memory Information
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SystemMemoryInfo {
    pub total_memory_mb: f64,
    pub available_memory_mb: f64,
    pub used_memory_mb: f64,
    pub memory_utilization_percent: f64,
}

#[cfg(target_os = "linux")]
pub fn get_system_memory_info() -> Option<SystemMemoryInfo> {
    use std::fs;
    
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0;
    let mut available_kb = 0;
    
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total_kb = line
                .split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()?;
        } else if line.starts_with("MemAvailable:") {
            available_kb = line
                .split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()?;
        }
    }
    
    let total_mb = total_kb as f64 / 1024.0;
    let available_mb = available_kb as f64 / 1024.0;
    let used_mb = total_mb - available_mb;
    let utilization = (used_mb / total_mb) * 100.0;
    
    Some(SystemMemoryInfo {
        total_memory_mb: total_mb,
        available_memory_mb: available_mb,
        used_memory_mb: used_mb,
        memory_utilization_percent: utilization,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn get_system_memory_info() -> Option<SystemMemoryInfo> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_tracker() {
        let tracker = MemoryTracker::new();
        
        tracker.record_allocation(1024);
        let stats = tracker.get_stats();
        assert_eq!(stats.current_heap_mb, 1024.0 / 1_048_576.0);
        assert_eq!(stats.total_allocations, 1);
        
        tracker.record_deallocation(512);
        let stats = tracker.get_stats();
        assert_eq!(stats.current_heap_mb, 512.0 / 1_048_576.0);
        assert_eq!(stats.total_deallocations, 1);
    }

    #[tokio::test]
    async fn test_hotspot_tracker() {
        let tracker = HotSpotTracker::new();
        
        tracker.record_hotspot("test_function", "test.rs:10", 1024).await;
        tracker.record_hotspot("test_function", "test.rs:10", 2048).await;
        
        let hotspots = tracker.get_top_hotspots(10).await;
        assert_eq!(hotspots.len(), 1);
        assert_eq!(hotspots[0].allocation_count, 2);
        assert_eq!(hotspots[0].total_bytes, 3072);
    }
}
