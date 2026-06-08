use crate::services::FleetParams;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRebalancingJob {
    pub affected_routes: Vec<String>,
    pub traffic_delay_minutes: i32,
    pub trigger_reason: String,
}

impl RouteRebalancingJob {
    pub fn new(
        affected_routes: Vec<String>,
        traffic_delay_minutes: i32,
        trigger_reason: String,
    ) -> Self {
        Self {
            affected_routes,
            traffic_delay_minutes,
            trigger_reason,
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
        println!(
            "🛣️  ROUTE REBALANCING JOB EXECUTING | tenant={} | routes={:?} | reason={}",
            ctx.tenant_id, self.affected_routes, self.trigger_reason
        );

        let tenant_ctx = TenantContext::new(ctx.tenant_id.clone());
        let params = FleetParams::default();

        let operations_service = ctx
            .app
            .service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // TypeDBAdapter::write() expects { "query": "insert ..." } — a raw TypeQL insert
        let operation_id = format!("rebal-{}", chrono::Utc::now().timestamp_millis());
        let event_time = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let rebalancing_event = serde_json::json!({
            "operation-id": &operation_id,
            "status": "completed",
            "query": format!(
                "insert $e isa operation-event, has id \"{op_id}\", has operation-id \"{op_id}\", has job-type \"route_rebalancing\", has event-status \"completed\", has event-time {ts};",
                op_id = operation_id,
                ts = event_time
            )
        });

        println!(
            "📝 ROUTE REBALANCING: Writing event to TypeDB for {} routes...",
            self.affected_routes.len()
        );

        operations_service
            .custom(tenant_ctx, "write", Some(rebalancing_event), params)
            .await
            .map_err(|e| {
                println!("❌ ROUTE REBALANCING FAILED: {}", e);
                JobError::Retryable(format!("Failed to record rebalancing event: {}", e))
            })?;

        let result = format!(
            "Route rebalancing event recorded for {} routes",
            self.affected_routes.len()
        );
        println!("✅ ROUTE REBALANCING COMPLETED: {}", result);
        Ok(result)
    }
}
