use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use dog_core::tenant::TenantContext;
use dog_axum::AxumApp;
use serde_json::Value;
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

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for GPSTrackingJob {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "gps_tracking";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;
        
        let query = serde_json::json!({
            "match": format!("$op isa operation, has operation-id '{}';", self.assignment_id),
            "get": "$op;"
        });
        
        operations_service
            .custom(tenant_ctx.clone(), "read", Some(query), params.clone())
            .await
            .map_err(|e| JobError::Permanent(format!("Failed to get assignment: {}", e)))?;
        
        let update_data = serde_json::json!({
            "assignment_id": self.assignment_id,
            "gps_update_timestamp": chrono::Utc::now().to_rfc3339(),
            "status": "tracking_updated"
        });
        
        operations_service
            .create(tenant_ctx, update_data, params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to update GPS timestamp: {}", e)))?;
        
        Ok(format!("GPS tracking completed for assignment: {}", self.assignment_id))
    }
}
