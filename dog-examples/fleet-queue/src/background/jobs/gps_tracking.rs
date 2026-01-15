use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use dog_core::tenant::TenantContext;
use crate::services::FleetParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GPSTrackingJob {
    pub assignment_id: String,
}

impl GPSTrackingJob {
    pub fn new(assignment_id: String) -> Self {
        Self { assignment_id }
    }
}

#[async_trait]
impl Job for GPSTrackingJob {
    type Context = crate::background::FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "gps_tracking";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        println!("🚀 GPS JOB EXECUTING for assignment: {}", self.assignment_id);
        
        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;
        
        // Create GPS tracking record using proper TypeDB write method with unique ID
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let unique_id = format!("GPS_{}_{}", self.assignment_id, chrono::Utc::now().timestamp_millis());
        let gps_query = format!(
            "insert $gps isa operation, has id \"{}\", has operation-id \"{}\", has assignment-id \"{}\", has job-type \"gps_update\", has status \"operational\", has timestamp {};",
            unique_id, unique_id, self.assignment_id, timestamp
        );
        
        let gps_update = serde_json::json!({
            "query": gps_query
        });
        
        println!("📝 GPS JOB: Writing to database for {}", self.assignment_id);
        
        operations_service
            .custom(tenant_ctx, "write", Some(gps_update), params)
            .await
            .map_err(|e| {
                println!("❌ GPS JOB FAILED for {}: {}", self.assignment_id, e);
                JobError::Retryable(format!("Failed to create GPS update: {}", e))
            })?;
        
        println!("✅ GPS JOB COMPLETED for assignment: {}", self.assignment_id);
        Ok(format!("GPS tracking completed for assignment: {}", self.assignment_id))
    }
}
