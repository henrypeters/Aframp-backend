use crate::compliance_effectiveness::repository::ComplianceEffectivenessRepository;
use crate::compliance_effectiveness::service::ReportGenerationService;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

pub struct ComplianceReportWorker {
    service: Arc<ReportGenerationService>,
    repo: Arc<ComplianceEffectivenessRepository>,
    interval: Duration,
}

impl ComplianceReportWorker {
    pub fn new(
        service: Arc<ReportGenerationService>,
        repo: Arc<ComplianceEffectivenessRepository>,
    ) -> Self {
        Self {
            service,
            repo,
            interval: Duration::from_secs(60 * 60), // hourly refresh
        }
    }

    pub fn start(self) {
        tokio::spawn(async move {
            info!("Compliance effectiveness worker started");
            let mut ticker = time::interval(self.interval);

            loop {
                ticker.tick().await;

                if let Err(e) = self.service.refresh_hourly_snapshot().await {
                    error!(error = %e, "failed to refresh hourly compliance KPI snapshot");
                }

                if let Err(e) = self.service.ensure_quarterly_report_current().await {
                    error!(error = %e, "failed to ensure quarterly compliance report");
                }

                if let Err(e) = self
                    .repo
                    .sample_dismissed_alerts_for_qc(0.05, "senior_compliance_queue")
                    .await
                {
                    error!(error = %e, "failed to run AML QC sampling");
                }
            }
        });
    }
}
