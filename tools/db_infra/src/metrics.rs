use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

pub fn init_metrics() -> PrometheusHandle {
    let builder = PrometheusBuilder::new();
    builder.install().expect("failed to install metrics recorder")
}
