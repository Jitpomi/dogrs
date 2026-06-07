use crate::services::FleetParams;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceSchedulingJob {
    pub vehicle_id: String,
    pub maintenance_type: String,
    pub current_mileage: i32,
    pub maintenance_threshold: i32,
}

impl MaintenanceSchedulingJob {
    pub fn new(
        vehicle_id: String,
        maintenance_type: String,
        current_mileage: i32,
        maintenance_threshold: i32,
    ) -> Self {
        Self {
            vehicle_id,
            maintenance_type,
            current_mileage,
            maintenance_threshold,
        }
    }
}

#[async_trait]
impl Job for MaintenanceSchedulingJob {
    type Context = crate::background::FleetContext;
    type Result = String;

    const JOB_TYPE: &'static str = "maintenance_scheduling";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();

        let operations_service = ctx
            .app
            .service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        let event_id = format!("maint-{}-{}", self.vehicle_id, chrono::Utc::now().timestamp_millis());
        let event_time = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let query = serde_json::json!({
            "operation-id": &event_id,
            "status": "completed",
            "query": format!(
                "insert $e isa operation-event, has id \"{id}\", has operation-id \"{id}\", has job-type \"maintenance_scheduling\", has event-status \"completed\", has event-time {ts};",
                id = event_id,
                ts = event_time
            )
        });

        operations_service
            .custom(tenant_ctx, "write", Some(query), params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to record maintenance event: {}", e)))?;

        Ok(format!("Maintenance event recorded for vehicle: {}", self.vehicle_id))
    }
}
