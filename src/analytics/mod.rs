//! Consumer Usage Analytics & Reporting System

pub mod anomaly;
pub mod cache;
pub mod handlers;
pub mod health;
pub mod metrics;
pub mod models;
pub mod reports;
pub mod repository;
pub mod routes;
pub mod snapshot;
mod tests;
pub mod worker;

pub use anomaly::AnomalyDetector;
pub use handlers::*;
pub use health::HealthScoreCalculator;
pub use models::*;
pub use reports::ReportGenerator;
pub use repository::AnalyticsRepository;
pub use routes::analytics_routes;
pub use snapshot::SnapshotGenerator;
