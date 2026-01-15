use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use dog_core::tenant::TenantContext;
use crate::services::FleetParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeAssignmentJob {
    pub route_id: String,
    pub pickup_location: (f64, f64),
    pub delivery_priority: String,
    pub required_certifications: Vec<String>,
}

impl EmployeeAssignmentJob {
    pub fn new(route_id: String, pickup_location: (f64, f64), delivery_priority: String, required_certifications: Vec<String>) -> Self {
        Self { 
            route_id, 
            pickup_location, 
            delivery_priority, 
            required_certifications 
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
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Simple assignment request record - TypeDB functions handle all employee selection logic
        let assignment_request = serde_json::json!({
            "route_id": self.route_id,
            "pickup_location": self.pickup_location,
            "delivery_priority": self.delivery_priority,
            "required_certifications": self.required_certifications,
            "request_timestamp": chrono::Utc::now().to_rfc3339()
        });

        operations_service
            .create(tenant_ctx, assignment_request, params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to create assignment request: {}", e)))?;

        Ok(format!("Assignment request created for route: {}", self.route_id))
    }
}
