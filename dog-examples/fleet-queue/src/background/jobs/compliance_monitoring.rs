use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use dog_core::tenant::TenantContext;
use dog_axum::AxumApp;
use serde_json::Value;
use crate::services::FleetParams;

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
            shift_start_time 
        }
    }
}

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for ComplianceMonitoringJob {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "compliance_monitoring";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Simple compliance monitoring record - TypeDB functions handle all business logic
        let monitoring_record = serde_json::json!({
            "employee_id": self.employee_id,
            "monitoring_type": self.monitoring_type,
            "shift_start_time": self.shift_start_time,
            "monitoring_timestamp": chrono::Utc::now().to_rfc3339()
        });

        operations_service
            .create(tenant_ctx, monitoring_record, params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to record monitoring: {}", e)))?;

        Ok(format!("Compliance monitoring completed for employee: {}", self.employee_id))
    }
}
