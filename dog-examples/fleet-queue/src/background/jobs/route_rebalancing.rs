use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use dog_core::tenant::TenantContext;
use crate::services::FleetParams;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRebalancingJob {
    pub affected_routes: Vec<String>,
    pub traffic_delay_minutes: i32,
    pub trigger_reason: String,
}

impl RouteRebalancingJob {
    pub fn new(affected_routes: Vec<String>, traffic_delay_minutes: i32, trigger_reason: String) -> Self {
        Self { 
            affected_routes, 
            traffic_delay_minutes, 
            trigger_reason 
        }
    }
}


#[async_trait]
impl Job for RouteRebalancingJob {
    type Context = crate::background::FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "route_rebalancing";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 2;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Simple rebalancing event record - TypeDB functions handle all business logic
        let rebalancing_event = serde_json::json!({
            "affected_routes": self.affected_routes,
            "traffic_delay_minutes": self.traffic_delay_minutes,
            "trigger_reason": self.trigger_reason,
            "event_timestamp": chrono::Utc::now().to_rfc3339()
        });

        operations_service
            .create(tenant_ctx, rebalancing_event, params)
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to record rebalancing event: {}", e)))?;

        Ok(format!("Route rebalancing event recorded for {} routes", self.affected_routes.len()))
    }
}
