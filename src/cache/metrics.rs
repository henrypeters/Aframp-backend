//! Prometheus metrics for the multi-level cache.
//!
//! Exposes per-category hit/miss/eviction/size counters for both L1 and L2.
//! Alert threshold: if L2 hit rate drops below CACHE_HIT_RATE_ALERT_THRESHOLD
//! (default 0.5), a warning is logged — wire this to your alerting system.

use prometheus::{
    register_counter_vec, register_gauge_vec, register_int_counter_vec, CounterVec, GaugeVec,
    IntCounterVec, Registry,
};
use std::sync::Arc;
use tracing::warn;

/// Metrics for the Level 1 in-process cache.
pub struct L1Metrics {
    hits: IntCounterVec,
    misses: IntCounterVec,
    inserts: IntCounterVec,
}

impl L1Metrics {
    pub fn new(registry: &Registry) -> Arc<Self> {
        let hits = IntCounterVec::new(
            prometheus::opts!("cache_l1_hits_total", "L1 cache hit count per category"),
            &["category"],
        )
        .expect("metric creation failed");

        let misses = IntCounterVec::new(
            prometheus::opts!("cache_l1_misses_total", "L1 cache miss count per category"),
            &["category"],
        )
        .expect("metric creation failed");

        let inserts = IntCounterVec::new(
            prometheus::opts!(
                "cache_l1_inserts_total",
                "L1 cache insert count per category"
            ),
            &["category"],
        )
        .expect("metric creation failed");

        registry.register(Box::new(hits.clone())).ok();
        registry.register(Box::new(misses.clone())).ok();
        registry.register(Box::new(inserts.clone())).ok();

        Arc::new(Self {
            hits,
            misses,
            inserts,
        })
    }

    pub fn record_hit(&self, category: &str) {
        self.hits.with_label_values(&[category]).inc();
        // #459 — tier="l1"
        if let Ok(m) = crate::metrics::cache::cache_hit_ratio_total()
            .check_with_label_values(&["l1", category]) {
            m.inc();
        }
    }

    pub fn record_miss(&self, category: &str) {
        self.misses.with_label_values(&[category]).inc();
    }

    pub fn record_insert(&self, category: &str) {
        self.inserts.with_label_values(&[category]).inc();
    }
}

/// Metrics for the Level 2 Redis cache.
pub struct L2Metrics {
    hits: IntCounterVec,
    misses: IntCounterVec,
    /// Tracks total requests per category for hit-rate calculation.
    requests: IntCounterVec,
    /// Alert threshold (0.0–1.0). Default 0.80 per #459.
    alert_threshold: f64,
    /// 5-minute rolling window ring buffer: (timestamp_secs, is_hit)
    rolling: std::sync::Mutex<std::collections::VecDeque<(u64, bool)>>,
}

impl L2Metrics {
    pub fn new(registry: &Registry) -> Arc<Self> {
        let hits = IntCounterVec::new(
            prometheus::opts!(
                "cache_l2_hits_total",
                "L2 Redis cache hit count per category"
            ),
            &["category"],
        )
        .expect("metric creation failed");

        let misses = IntCounterVec::new(
            prometheus::opts!(
                "cache_l2_misses_total",
                "L2 Redis cache miss count per category"
            ),
            &["category"],
        )
        .expect("metric creation failed");

        let requests = IntCounterVec::new(
            prometheus::opts!(
                "cache_l2_requests_total",
                "L2 Redis cache total requests per category"
            ),
            &["category"],
        )
        .expect("metric creation failed");

        registry.register(Box::new(hits.clone())).ok();
        registry.register(Box::new(misses.clone())).ok();
        registry.register(Box::new(requests.clone())).ok();

        // Default 80 % per #459 acceptance criteria; configurable via env
        let alert_threshold = std::env::var("CACHE_HIT_RATE_ALERT_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.80);

        Arc::new(Self {
            hits,
            misses,
            requests,
            alert_threshold,
            rolling: std::sync::Mutex::new(std::collections::VecDeque::with_capacity(10_000)),
        })
    }

    pub fn record_hit(&self, category: &str) {
        self.hits.with_label_values(&[category]).inc();
        self.requests.with_label_values(&[category]).inc();
        self.push_rolling(true);
        // #459 — increment per-namespace hit ratio counter (use rate() in Prometheus)
        if let Ok(m) = crate::metrics::cache::cache_hit_ratio_total()
            .check_with_label_values(&["l2", category]) {
            m.inc();
        }
    }

    pub fn record_miss(&self, category: &str) {
        self.misses.with_label_values(&[category]).inc();
        self.requests.with_label_values(&[category]).inc();
        self.push_rolling(false);
        self.check_rolling_alert(category);
    }

    fn push_rolling(&self, is_hit: bool) {
        if let Ok(mut deq) = self.rolling.lock() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            deq.push_back((now, is_hit));
            // Bound memory: drop entries older than 6 minutes
            let cutoff = now.saturating_sub(360);
            while deq.front().map_or(false, |(t, _)| *t < cutoff) {
                deq.pop_front();
            }
        }
    }

    /// Returns the hit ratio for the 5-minute rolling window, or `None` if
    /// sample is too small (<20 events).
    pub fn rolling_hit_ratio_5m(&self) -> Option<f64> {
        let Ok(deq) = self.rolling.lock() else { return None };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cutoff = now.saturating_sub(300); // 5 min

        let (hits, total) = deq.iter()
            .filter(|(t, _)| *t >= cutoff)
            .fold((0u64, 0u64), |(h, t), (_, is_hit)| {
                if *is_hit { (h + 1, t + 1) } else { (h, t + 1) }
            });

        if total < 20 { None } else { Some(hits as f64 / total as f64) }
    }

    /// Aggregate hit count across all categories via Prometheus gather.
    /// Used only for the stats snapshot endpoint.
    pub fn hits_count(&self, _category: &str) -> u64 {
        use prometheus::Encoder;
        let families = self.hits.collect();
        families.iter().flat_map(|mf| {
            mf.get_metric().iter().map(|m| m.get_counter().get_value() as u64)
        }).sum()
    }

    pub fn misses_count(&self, _category: &str) -> u64 {
        let families = self.misses.collect();
        families.iter().flat_map(|mf| {
            mf.get_metric().iter().map(|m| m.get_counter().get_value() as u64)
        }).sum()
    }

    fn check_rolling_alert(&self, category: &str) {
        if let Some(rate) = self.rolling_hit_ratio_5m() {
            if rate < self.alert_threshold {
                warn!(
                    category,
                    hit_rate_5m = rate,
                    threshold = self.alert_threshold,
                    "ALERT: L2 cache hit rate below {}% over 5-minute window", (self.alert_threshold * 100.0) as u32
                );
            }
        }
    }
}

/// Gauge metrics for cache sizes (updated periodically by the warmer).
pub struct CacheSizeMetrics {
    l1_size: GaugeVec,
}

impl CacheSizeMetrics {
    pub fn new(registry: &Registry) -> Arc<Self> {
        let l1_size = GaugeVec::new(
            prometheus::opts!("cache_l1_size_entries", "Current L1 cache entry count"),
            &["category"],
        )
        .expect("metric creation failed");

        registry.register(Box::new(l1_size.clone())).ok();

        Arc::new(Self { l1_size })
    }

    pub fn set_l1_size(&self, category: &str, count: u64) {
        self.l1_size
            .with_label_values(&[category])
            .set(count as f64);
    }
}
