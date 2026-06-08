use crate::services::FleetParams;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceMonitoringJob {
    pub employee_id: String,
    pub monitoring_type: String,
    pub shift_start_time: String,
}

impl ComplianceMonitoringJob {
    pub fn new(employee_id: String, monitoring_type: String, shift_start_time: String) -> Self {
        Self {
            employee_id,
            monitoring_type,
            shift_start_time,
        }
    }
}

#[async_trait]
impl Job for ComplianceMonitoringJob {
    type Context = crate::background::FleetContext;
    type Result = String;

    const JOB_TYPE: &'static str = "compliance_monitoring";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();

        let operations_service = ctx
            .app
            .service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        let event_id = format!(
            "comply-{}-{}",
            self.employee_id,
            chrono::Utc::now().timestamp_millis()
        );
        let event_time = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let query = serde_json::json!({
            "operation-id": &event_id,
            "status": "completed",
            "query": format!(
                "insert $e isa operation-event, has id \"{id}\", has operation-id \"{id}\", has job-type \"compliance_monitoring\", has event-status \"completed\", has event-time {ts};",
                id = event_id,
                ts = event_time
            )
        });

        operations_service
            .custom(tenant_ctx, "write", Some(query), params)
            .await
            .map_err(|e| {
                JobError::Retryable(format!("Failed to record compliance event: {}", e))
            })?;

        Ok(format!(
            "Compliance event recorded for employee: {}",
            self.employee_id
        ))
    }
}
