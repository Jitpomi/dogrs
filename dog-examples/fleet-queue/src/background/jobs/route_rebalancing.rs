use async_trait::async_trait;
use dog_queue::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use dog_core::tenant::TenantContext;
use dog_axum::AxumApp;
use serde_json::{json, Value};
use crate::services::FleetParams;

/// Helper function to get configuration value from TypeDB rules with fallback to app state
async fn get_config_value(ctx: &FleetContext, key: &str, default: &str) -> String {
    // 1. Try to get from TypeDB rules (highest priority)
    if let Ok(rules_service) = ctx.app.app.service("rules") {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let query_data = json!({
            "rule_name": key
        });
        
        if let Ok(result) = rules_service.custom(tenant_ctx, "get_config_value", Some(query_data), params).await {
            if !result.is_null() {
                if let Some(value) = result.as_str() {
                    return value.to_string();
                }
            }
        }
    }
    
    // 2. Try app state (fallback)
    if let Some(app_value) = ctx.app.app.get(key) {
        return app_value;
    }
    
    // 3. Use default value
    default.to_string()
}

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

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for RouteRebalancingJob {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "route_rebalancing";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 2;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;
        
        let tomtom_service = ctx.app.app.service("tomtom")
            .map_err(|e| JobError::Permanent(format!("TomTom service not found: {}", e)))?;
        
        let deliveries_service = ctx.app.app.service("deliveries")
            .map_err(|e| JobError::Permanent(format!("Deliveries service not found: {}", e)))?;

        let mut rebalanced_routes = 0;
        let mut notifications_sent = 0;

        // Get configurable traffic delay trigger - try TypeDB rules first, then app state
        let traffic_delay_trigger: i32 = get_config_value(&ctx, "rebalancing.traffic_delay_trigger_minutes", "30").await.parse().unwrap_or(30);

        // Only proceed if delay is significant
        if self.traffic_delay_minutes > traffic_delay_trigger {
            // Get all active deliveries for affected routes
            for route_id in &self.affected_routes {
                let active_deliveries_query = serde_json::json!({
                    "match": format!("$delivery isa delivery, has route-id '{}', has status 'active';", route_id),
                    "get": "$delivery;"
                });

                let active_deliveries = operations_service
                    .custom(tenant_ctx.clone(), "read", Some(active_deliveries_query), params.clone())
                    .await
                    .map_err(|e| JobError::Retryable(format!("Failed to get active deliveries for route {}: {}", route_id, e)))?;

                if let Some(deliveries_array) = active_deliveries.as_array() {
                    for delivery in deliveries_array {
                        let delivery_id = delivery.get("delivery-id").and_then(|v| v.as_str()).unwrap_or("unknown");
                        let customer_priority = delivery.get("customer-priority").and_then(|v| v.as_str()).unwrap_or("standard");
                        let current_lat = delivery.get("current-lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let current_lng = delivery.get("current-lng").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let dest_lat = delivery.get("dest-lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let dest_lng = delivery.get("dest-lng").and_then(|v| v.as_f64()).unwrap_or(0.0);

                        // Calculate new ETA with current traffic
                        let updated_eta = tomtom_service
                            .custom(tenant_ctx.clone(), "eta", Some(serde_json::json!({
                                "vehicle_id": delivery.get("vehicle-id").and_then(|v| v.as_str()).unwrap_or("unknown"),
                                "delivery_id": delivery_id,
                                "current_lat": current_lat,
                                "current_lng": current_lng,
                                "dest_lat": dest_lat,
                                "dest_lng": dest_lng
                            })), params.clone())
                            .await
                            .map_err(|e| JobError::Retryable(format!("Failed to calculate updated ETA for delivery {}: {}", delivery_id, e)))?;

                        let new_eta = updated_eta.get("estimated_arrival").and_then(|v| v.as_str()).unwrap_or("");
                        let remaining_time = updated_eta.get("remaining_time_seconds").and_then(|v| v.as_i64()).unwrap_or(0);

                        // Get configurable reassignment thresholds - try TypeDB rules first, then app state
                        let premium_threshold: i64 = get_config_value(&ctx, "rebalancing.premium_threshold_seconds", "7200").await.parse().unwrap_or(7200);
                        let priority_threshold: i64 = get_config_value(&ctx, "rebalancing.priority_threshold_seconds", "10800").await.parse().unwrap_or(10800);
                        let standard_threshold: i64 = get_config_value(&ctx, "rebalancing.standard_threshold_seconds", "14400").await.parse().unwrap_or(14400);

                        // Check if we need to reassign based on priority and delay
                        let should_reassign = match customer_priority {
                            "premium" => remaining_time > premium_threshold,
                            "priority" => remaining_time > priority_threshold,
                            _ => remaining_time > standard_threshold,
                        };

                        if should_reassign {
                            // Find alternative drivers for reassignment
                            let alternative_drivers_query = serde_json::json!({
                                "match": "$driver isa employee, has role 'driver', has status 'available';",
                                "get": "$driver;"
                            });

                            let alternative_drivers = operations_service
                                .custom(tenant_ctx.clone(), "read", Some(alternative_drivers_query), params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to get alternative drivers: {}", e)))?;

                            if let Some(drivers_array) = alternative_drivers.as_array() {
                                let mut best_alternative: Option<(String, i64)> = None;

                                for alt_driver in drivers_array {
                                    let alt_driver_id = alt_driver.get("driver-id").and_then(|v| v.as_str()).unwrap_or("unknown");
                                    let alt_current_lat = alt_driver.get("current-lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                    let alt_current_lng = alt_driver.get("current-lng").and_then(|v| v.as_f64()).unwrap_or(0.0);

                                    // Calculate ETA for alternative driver
                                    let alt_eta = tomtom_service
                                        .custom(tenant_ctx.clone(), "eta", Some(serde_json::json!({
                                            "vehicle_id": "alternative",
                                            "delivery_id": delivery_id,
                                            "current_lat": alt_current_lat,
                                            "current_lng": alt_current_lng,
                                            "dest_lat": dest_lat,
                                            "dest_lng": dest_lng
                                        })), params.clone())
                                        .await;

                                    if let Ok(alt_eta_result) = alt_eta {
                                        let alt_remaining_time = alt_eta_result.get("remaining_time_seconds").and_then(|v| v.as_i64()).unwrap_or(999999);
                                        
                                        if alt_remaining_time < remaining_time {
                                            if let Some((_, current_best_time)) = &best_alternative {
                                                if alt_remaining_time < *current_best_time {
                                                    best_alternative = Some((alt_driver_id.to_string(), alt_remaining_time));
                                                }
                                            } else {
                                                best_alternative = Some((alt_driver_id.to_string(), alt_remaining_time));
                                            }
                                        }
                                    }
                                }

                                // Reassign to best alternative if found
                                if let Some((best_driver_id, best_time)) = best_alternative {
                                    let reassignment_data = serde_json::json!({
                                        "delivery_id": delivery_id,
                                        "old_route_id": route_id,
                                        "new_driver_id": best_driver_id,
                                        "reassignment_reason": format!("Traffic delay: {} minutes", self.traffic_delay_minutes),
                                        "new_eta": chrono::Utc::now() + chrono::Duration::seconds(best_time),
                                        "reassignment_timestamp": chrono::Utc::now().to_rfc3339()
                                    });

                                    deliveries_service
                                        .create(tenant_ctx.clone(), reassignment_data, params.clone())
                                        .await
                                        .map_err(|e| JobError::Retryable(format!("Failed to reassign delivery {}: {}", delivery_id, e)))?;

                                    rebalanced_routes += 1;
                                }
                            }
                        }

                        // Send customer notification with updated ETA
                        let notification_data = serde_json::json!({
                            "delivery_id": delivery_id,
                            "customer_priority": customer_priority,
                            "updated_eta": new_eta,
                            "delay_reason": self.trigger_reason,
                            "notification_type": "eta_update",
                            "timestamp": chrono::Utc::now().to_rfc3339()
                        });

                        // Store notification (would integrate with notification service)
                        operations_service
                            .create(tenant_ctx.clone(), notification_data, params.clone())
                            .await
                            .map_err(|e| JobError::Retryable(format!("Failed to create notification for delivery {}: {}", delivery_id, e)))?;

                        notifications_sent += 1;
                    }
                }
            }
        }

        Ok(format!("Route rebalancing completed: {} routes rebalanced, {} notifications sent", 
                  rebalanced_routes, notifications_sent))
    }
}
