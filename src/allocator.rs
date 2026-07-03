// ============================================================================
// Alternative Memory Allocator Configuration
// ============================================================================
// Supports jemalloc and mimalloc for improved performance under high
// concurrent throughput and reduced memory fragmentation.
// ============================================================================

// Use jemalloc if enabled via feature flag
#[cfg(all(feature = "jemalloc", not(target_env = "msvc")))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// Use mimalloc if enabled via feature flag
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Get the name of the currently active allocator
pub fn get_allocator_name() -> &'static str {
    #[cfg(all(feature = "jemalloc", not(target_env = "msvc")))]
    return "jemalloc";
    
    #[cfg(feature = "mimalloc")]
    return "mimalloc";
    
    #[cfg(not(any(
        all(feature = "jemalloc", not(target_env = "msvc")),
        feature = "mimalloc"
    )))]
    return "system";
}

/// Get allocator statistics if available
#[cfg(all(feature = "jemalloc", not(target_env = "msvc")))]
pub fn get_allocator_stats() -> Option<String> {
    use tikv_jemallocator::Jemalloc;
    
    // jemalloc provides extensive statistics
    Some(format!(
        "allocator=jemalloc, stats_available=true"
    ))
}

#[cfg(not(all(feature = "jemalloc", not(target_env = "msvc"))))]
pub fn get_allocator_stats() -> Option<String> {
    Some(format!("allocator={}, stats_available=false", get_allocator_name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocator_name() {
        let name = get_allocator_name();
        assert!(
            name == "jemalloc" || name == "mimalloc" || name == "system",
            "Unexpected allocator name: {}",
            name
        );
    }

    #[test]
    fn test_allocator_stats() {
        let stats = get_allocator_stats();
        assert!(stats.is_some());
    }
}
