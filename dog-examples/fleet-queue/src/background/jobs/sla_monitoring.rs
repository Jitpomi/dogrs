use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use dog_core::tenant::TenantContext;
use crate::services::FleetParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SLAMonitoringJob {
    pub delivery_id: String,
    pub customer_tier: String,
    pub promised_delivery_time: String,
    pub sla_threshold_minutes: i32,
}

impl SLAMonitoringJob {
    pub fn new(delivery_id: String, customer_tier: String, promised_delivery_time: String, sla_threshold_minutes: i32) -> Self {
        Self { 
            delivery_id, 
            customer_tier, 
            promised_delivery_time, 
            sla_threshold_minutes 
        }
    }
}


#[async_trait]
impl Job for SLAMonitoringJob {
    type Context = crate::background::FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "sla_monitoring";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Simple SLA monitoring record - TypeDB functions handle all SLA logic
        let sla_record = serde_json::json!({
            "delivery_id": self.delivery_id,
            "customer_tier": self.customer_tier,
            "promised_delivery_time": self.promised_delivery_time,
            "sla_threshold_minutes": self.sla_threshold_minutes,
            "monitoring_timestamp": chrono::Utc::now().to_rfc3339()
        });

        operations_service
            .create(tenant_ctx, sla_record, params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to create SLA monitoring record: {}", e)))?;

        Ok(format!("SLA monitoring completed for delivery: {}", self.delivery_id))
    }
}
