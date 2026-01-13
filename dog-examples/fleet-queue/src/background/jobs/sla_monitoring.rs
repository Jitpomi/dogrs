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

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for SLAMonitoringJob {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "sla_monitoring";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let deliveries_service = ctx.app.app.service("deliveries")
            .map_err(|e| JobError::Permanent(format!("Deliveries service not found: {}", e)))?;
        
        let tomtom_service = ctx.app.app.service("tomtom")
            .map_err(|e| JobError::Permanent(format!("TomTom service not found: {}", e)))?;
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        let employees_service = ctx.app.app.service("employees")
            .map_err(|e| JobError::Permanent(format!("Employees service not found: {}", e)))?;

        // Get delivery details
        let delivery_query = serde_json::json!({
            "match": format!("$delivery isa delivery, has delivery-id '{}';", self.delivery_id),
            "get": "$delivery;"
        });

        let delivery_result = deliveries_service
            .custom(tenant_ctx.clone(), "read", Some(delivery_query), params.clone())
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to get delivery details: {}", e)))?;

        if let Some(delivery_array) = delivery_result.as_array() {
            if let Some(delivery) = delivery_array.first() {
                let current_lat = delivery.get("current-lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let current_lng = delivery.get("current-lng").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let dest_lat = delivery.get("dest-lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let dest_lng = delivery.get("dest-lng").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let vehicle_id = delivery.get("vehicle-id").and_then(|v| v.as_str()).unwrap_or("unknown");
                let driver_id = delivery.get("driver-id").and_then(|v| v.as_str()).unwrap_or("unknown");
                let status = delivery.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");

                // Only monitor active deliveries for SLA compliance
                if status != "active" {
                    return Ok(format!("Delivery {} is not active (status: {}), skipping SLA monitoring", self.delivery_id, status));
                }

                // Calculate current ETA using TomTom
                let eta_result = tomtom_service
                    .custom(tenant_ctx.clone(), "eta", Some(serde_json::json!({
                        "vehicle_id": vehicle_id,
                        "delivery_id": self.delivery_id,
                        "current_lat": current_lat,
                        "current_lng": current_lng,
                        "dest_lat": dest_lat,
                        "dest_lng": dest_lng
                    })), params.clone())
                    .await
                    .map_err(|e| JobError::Retryable(format!("Failed to calculate ETA: {}", e)))?;

                let estimated_arrival = eta_result.get("estimated_arrival").and_then(|v| v.as_str()).unwrap_or("");
                let remaining_time_seconds = eta_result.get("remaining_time_seconds").and_then(|v| v.as_i64()).unwrap_or(0);

                // Check if delivery is severely delayed (for priority escalation)
                let severe_delay_threshold = get_config_value(&ctx, "sla.severe_delay_threshold_seconds", "3600").await.parse().unwrap_or(3600);
                let is_severely_delayed = remaining_time_seconds > severe_delay_threshold;

                // Parse promised delivery time
                let promised_time = chrono::DateTime::parse_from_rfc3339(&self.promised_delivery_time)
                    .map_err(|e| JobError::Permanent(format!("Invalid promised delivery time format: {}", e)))?;

                let estimated_time = chrono::DateTime::parse_from_rfc3339(estimated_arrival)
                    .map_err(|e| JobError::Retryable(format!("Invalid ETA format from TomTom: {}", e)))?;

                // Calculate delay risk in minutes
                let delay_minutes = (estimated_time - promised_time).num_minutes();
                let delay_risk_minutes = delay_minutes.max(0) as i32;

                let mut actions_taken = Vec::new();

                // Execute SLA actions
                let sla_actions = self.evaluate_sla_breach(delay_risk_minutes, &ctx).await;
                for action in sla_actions {
                    // Add severe delay flag for high-priority escalations
                    let action_with_context = if is_severely_delayed && action == "escalate" {
                        "escalate_severe_delay"
                    } else {
                        &action
                    };
                    match action_with_context {
                        "escalate" => {
                            let escalation_data = serde_json::json!({
                                "delivery_id": self.delivery_id,
                                "customer_tier": self.customer_tier,
                                "delay_risk_minutes": delay_risk_minutes,
                                "escalation_level": "high",
                                "escalation_timestamp": chrono::Utc::now().to_rfc3339(),
                                "escalation_reason": "SLA breach risk detected"
                            });

                            operations_service
                                .create(tenant_ctx.clone(), escalation_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to create escalation: {}", e)))?;

                            actions_taken.push("escalated".to_string());
                        },
                        "escalate_severe_delay" => {
                            let escalation_data = serde_json::json!({
                                "delivery_id": self.delivery_id,
                                "customer_tier": self.customer_tier,
                                "delay_risk_minutes": delay_risk_minutes,
                                "remaining_time_seconds": remaining_time_seconds,
                                "escalation_level": "critical",
                                "escalation_timestamp": chrono::Utc::now().to_rfc3339(),
                                "escalation_reason": "Severe delay detected - over 1 hour behind schedule"
                            });

                            operations_service
                                .create(tenant_ctx.clone(), escalation_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to create escalation: {}", e)))?;

                            actions_taken.push("escalated".to_string());
                        },
                        "reassign_to_top_driver" => {
                            // Find top-rated available driver
                            let top_drivers_query = serde_json::json!({
                                "match": "$driver isa employee, has role 'driver', has status 'available', has performance-rating > 4.0;",
                                "get": "$driver;"
                            });

                            let top_drivers = employees_service
                                .custom(tenant_ctx.clone(), "read", Some(top_drivers_query), params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to get top drivers: {}", e)))?;

                            if let Some(drivers_array) = top_drivers.as_array() {
                                if let Some(top_driver) = drivers_array.first() {
                                    let top_driver_id = top_driver.get("driver-id").and_then(|v| v.as_str()).unwrap_or("unknown");

                                    let reassignment_data = serde_json::json!({
                                        "delivery_id": self.delivery_id,
                                        "original_driver_id": driver_id,
                                        "new_driver_id": top_driver_id,
                                        "reassignment_reason": "SLA breach prevention - assigned to top performer",
                                        "customer_tier": self.customer_tier,
                                        "reassignment_timestamp": chrono::Utc::now().to_rfc3339()
                                    });

                                    operations_service
                                        .create(tenant_ctx.clone(), reassignment_data, params.clone())
                                        .await
                                        .map_err(|e| JobError::Retryable(format!("Failed to reassign to top driver: {}", e)))?;

                                    actions_taken.push(format!("reassigned_to_top_driver_{}", top_driver_id));
                                }
                            }
                        },
                        "priority_routing" => {
                            // Request priority routing with traffic avoidance
                            let priority_route = tomtom_service
                                .custom(tenant_ctx.clone(), "route", Some(serde_json::json!({
                                    "from_lat": current_lat,
                                    "from_lng": current_lng,
                                    "to_lat": dest_lat,
                                    "to_lng": dest_lng,
                                    "route_type": "fastest",
                                    "avoid_traffic": true,
                                    "priority": "high"
                                })), params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to calculate priority route: {}", e)))?;

                            let route_update_data = serde_json::json!({
                                "delivery_id": self.delivery_id,
                                "route_type": "priority",
                                "route_data": priority_route,
                                "route_updated_timestamp": chrono::Utc::now().to_rfc3339()
                            });

                            operations_service
                                .create(tenant_ctx.clone(), route_update_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to update to priority route: {}", e)))?;

                            actions_taken.push("priority_routing_enabled".to_string());
                        },
                        "frequent_updates" => {
                            // Enable frequent GPS tracking updates (every 5 minutes instead of 60)
                            let tracking_update = serde_json::json!({
                                "delivery_id": self.delivery_id,
                                "tracking_frequency": "high",
                                "update_interval_seconds": 300,
                                "customer_tier": self.customer_tier,
                                "tracking_reason": "SLA monitoring - premium service",
                                "enabled_timestamp": chrono::Utc::now().to_rfc3339()
                            });

                            operations_service
                                .create(tenant_ctx.clone(), tracking_update, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to enable frequent updates: {}", e)))?;

                            actions_taken.push("frequent_updates_enabled".to_string());
                        },
                        "customer_notification" => {
                            // Send proactive customer notification
                            let customer_notification = serde_json::json!({
                                "delivery_id": self.delivery_id,
                                "notification_type": "proactive_eta_update",
                                "customer_tier": self.customer_tier,
                                "current_eta": estimated_arrival,
                                "delay_risk_minutes": delay_risk_minutes,
                                "message": format!("Your {} delivery is being closely monitored. Current ETA: {}", self.customer_tier, estimated_arrival),
                                "notification_timestamp": chrono::Utc::now().to_rfc3339()
                            });

                            operations_service
                                .create(tenant_ctx.clone(), customer_notification, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to send customer notification: {}", e)))?;

                            actions_taken.push("customer_notified".to_string());
                        },
                        _ => {}
                    }
                }

                // Record SLA monitoring result
                let sla_record = serde_json::json!({
                    "delivery_id": self.delivery_id,
                    "customer_tier": self.customer_tier,
                    "promised_time": self.promised_delivery_time,
                    "estimated_arrival": estimated_arrival,
                    "delay_risk_minutes": delay_risk_minutes,
                    "sla_status": if delay_risk_minutes <= self.sla_threshold_minutes { "on_track" } else { "at_risk" },
                    "actions_taken": actions_taken,
                    "monitoring_timestamp": chrono::Utc::now().to_rfc3339()
                });

                operations_service
                    .create(tenant_ctx, sla_record, params)
                    .await
                    .map_err(|e| JobError::Retryable(format!("Failed to record SLA monitoring: {}", e)))?;

                Ok(format!("SLA monitoring completed for delivery {}: {} minutes delay risk, actions: {:?}", 
                          self.delivery_id, delay_risk_minutes, actions_taken))
            } else {
                Err(JobError::Permanent(format!("Delivery {} not found", self.delivery_id)))
            }
        } else {
            Err(JobError::Permanent(format!("Invalid delivery query result for {}", self.delivery_id)))
        }
    }
}

impl SLAMonitoringJob {
    async fn evaluate_sla_breach(&self, delay_risk_minutes: i32, ctx: &FleetContext) -> Vec<String> {
        let mut actions = Vec::new();

        // Get configurable SLA thresholds - try TypeDB rules first, then app state
        let premium_escalation: i32 = get_config_value(ctx, "sla.premium.escalation_threshold_minutes", "15").await.parse().unwrap_or(15);
        let premium_notification: i32 = get_config_value(ctx, "sla.premium.notification_threshold_minutes", "5").await.parse().unwrap_or(5);
        let priority_escalation: i32 = get_config_value(ctx, "sla.priority.escalation_threshold_minutes", "30").await.parse().unwrap_or(30);
        let priority_notification: i32 = get_config_value(ctx, "sla.priority.notification_threshold_minutes", "15").await.parse().unwrap_or(15);
        let standard_escalation: i32 = get_config_value(ctx, "sla.standard.escalation_threshold_minutes", "60").await.parse().unwrap_or(60);
        let standard_notification: i32 = get_config_value(ctx, "sla.standard.notification_threshold_minutes", "30").await.parse().unwrap_or(30);

        match self.customer_tier.as_str() {
            "premium" => {
                if delay_risk_minutes > premium_escalation {
                    actions.push("escalate".to_string());
                    actions.push("reassign_to_top_driver".to_string());
                    actions.push("priority_routing".to_string());
                }
                if delay_risk_minutes > premium_notification {
                    actions.push("frequent_updates".to_string());
                    actions.push("customer_notification".to_string());
                }
            },
            "priority" => {
                if delay_risk_minutes > priority_escalation {
                    actions.push("escalate".to_string());
                    actions.push("priority_routing".to_string());
                }
                if delay_risk_minutes > priority_notification {
                    actions.push("frequent_updates".to_string());
                    actions.push("customer_notification".to_string());
                }
            },
            "standard" => {
                if delay_risk_minutes > standard_escalation {
                    actions.push("escalate".to_string());
                }
                if delay_risk_minutes > standard_notification {
                    actions.push("customer_notification".to_string());
                }
            },
            _ => {
                // Default handling for unknown tiers
                if delay_risk_minutes > 45 {
                    actions.push("customer_notification".to_string());
                }
            }
        }

        actions
    }
}
