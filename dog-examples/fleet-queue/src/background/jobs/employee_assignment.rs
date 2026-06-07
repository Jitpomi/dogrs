use crate::services::FleetParams;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeAssignmentJob {
    pub route_id: String,
    pub pickup_location: (f64, f64),
    pub delivery_priority: String,
    pub required_certifications: Vec<String>,
}

impl EmployeeAssignmentJob {
    pub fn new(
        route_id: String,
        pickup_location: (f64, f64),
        delivery_priority: String,
        required_certifications: Vec<String>,
    ) -> Self {
        Self {
            route_id,
            pickup_location,
            delivery_priority,
            required_certifications,
        }
    }
}

#[async_trait]
impl Job for EmployeeAssignmentJob {
    type Context = crate::background::FleetContext;
    type Result = String;

    const JOB_TYPE: &'static str = "employee_assignment";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();

        let operations_service = ctx
            .app
            .service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        let event_id = format!("assign-{}-{}", self.route_id, chrono::Utc::now().timestamp_millis());
        let event_time = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let query = serde_json::json!({
            "operation-id": &event_id,
            "status": "completed",
            "query": format!(
                "insert $e isa operation-event, has id \"{id}\", has operation-id \"{id}\", has job-type \"employee_assignment\", has event-status \"completed\", has event-time {ts};",
                id = event_id,
                ts = event_time
            )
        });

        operations_service
            .custom(tenant_ctx, "write", Some(query), params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to record assignment event: {}", e)))?;

        Ok(format!("Assignment event recorded for route: {}", self.route_id))
    }
}
