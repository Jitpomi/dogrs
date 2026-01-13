use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use dog_core::tenant::TenantContext;
use dog_axum::AxumApp;
use serde_json::Value;
use crate::services::FleetParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceSchedulingJob {
    pub vehicle_id: String,
    pub maintenance_type: String,
    pub current_mileage: i32,
    pub maintenance_threshold: i32,
}

impl MaintenanceSchedulingJob {
    pub fn new(vehicle_id: String, maintenance_type: String, current_mileage: i32, maintenance_threshold: i32) -> Self {
        Self { 
            vehicle_id, 
            maintenance_type, 
            current_mileage, 
            maintenance_threshold 
        }
    }
}

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for MaintenanceSchedulingJob {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "maintenance_scheduling";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Simple maintenance check record - TypeDB functions handle all business logic
        let maintenance_check = serde_json::json!({
            "vehicle_id": self.vehicle_id,
            "maintenance_type": self.maintenance_type,
            "current_mileage": self.current_mileage,
            "check_timestamp": chrono::Utc::now().to_rfc3339()
        });

        operations_service
            .create(tenant_ctx, maintenance_check, params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to record maintenance check: {}", e)))?;

        Ok(format!("Maintenance check completed for vehicle: {}", self.vehicle_id))
    }
}
