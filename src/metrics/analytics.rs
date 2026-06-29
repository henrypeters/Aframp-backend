//! Wallet analytics Prometheus metrics (Issue #369).

use prometheus::{register_counter_vec, register_gauge_vec, CounterVec, GaugeVec};
use std::sync::OnceLock;

static CACHE_MISSES: OnceLock<CounterVec> = OnceLock::new();
static SNAPSHOTS: OnceLock<CounterVec> = OnceLock::new();
static ANOMALY_FLAGGED: OnceLock<GaugeVec> = OnceLock::new();
static ACTIVE_WALLETS: OnceLock<GaugeVec> = OnceLock::new();
static AVG_RISK: OnceLock<GaugeVec> = OnceLock::new();

fn cache_misses() -> &'static CounterVec {
    CACHE_MISSES.get_or_init(|| {
        register_counter_vec!(
            "aframp_wallet_analytics_cache_misses_total",
            "Wallet analytics cache misses by endpoint",
            &["endpoint"]
        )
        .expect("register wallet analytics cache misses")
    })
}

fn snapshots() -> &'static CounterVec {
    SNAPSHOTS.get_or_init(|| {
        register_counter_vec!(
            "aframp_wallet_analytics_snapshots_total",
            "Wallet analytics snapshots generated",
            &["wallet_id", "scope"]
        )
        .expect("register wallet analytics snapshots")
    })
}

fn anomaly_flagged() -> &'static GaugeVec {
    ANOMALY_FLAGGED.get_or_init(|| {
        register_gauge_vec!(
            "aframp_wallet_analytics_anomaly_flagged_wallets",
            "Wallets currently flagged by anomaly detection",
            &[]
        )
        .expect("register wallet analytics anomaly gauge")
    })
}

fn active_wallets() -> &'static GaugeVec {
    ACTIVE_WALLETS.get_or_init(|| {
        register_gauge_vec!(
            "aframp_wallet_analytics_active_wallets",
            "Active wallets included in the latest analytics snapshot",
            &[]
        )
        .expect("register wallet analytics active wallets gauge")
    })
}

fn avg_risk() -> &'static GaugeVec {
    AVG_RISK.get_or_init(|| {
        register_gauge_vec!(
            "aframp_wallet_analytics_avg_risk_score",
            "Average wallet risk score from the latest snapshot run",
            &[]
        )
        .expect("register wallet analytics avg risk gauge")
    })
}

pub fn cache_miss(endpoint: &str) {
    cache_misses().with_label_values(&[endpoint]).inc();
}

pub fn snapshot_generated(wallet_id: &str, scope: &str) {
    snapshots().with_label_values(&[wallet_id, scope]).inc();
}

pub fn anomaly_flagged_wallets(count: f64) {
    anomaly_flagged().with_label_values(&[]).set(count);
}

pub fn active_wallet_count(count: f64) {
    active_wallets().with_label_values(&[]).set(count);
}

pub fn avg_risk_score(score: f64) {
    avg_risk().with_label_values(&[]).set(score);
}
