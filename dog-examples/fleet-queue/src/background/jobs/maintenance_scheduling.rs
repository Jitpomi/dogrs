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
        
        let vehicles_service = ctx.app.app.service("vehicles")
            .map_err(|e| JobError::Permanent(format!("Vehicles service not found: {}", e)))?;
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;
        
        let employees_service = ctx.app.app.service("employees")
            .map_err(|e| JobError::Permanent(format!("Employees service not found: {}", e)))?;

        // Get configurable maintenance threshold - try TypeDB rules first, then app state
        let maintenance_threshold = get_config_value(&ctx, "maintenance.default_mileage_threshold", &self.maintenance_threshold.to_string()).await.parse().unwrap_or(self.maintenance_threshold);
        
        // Check if maintenance is needed
        if self.current_mileage >= maintenance_threshold {
            // Update vehicle status to maintenance pending
            let vehicle_update = serde_json::json!({
                "vehicle_id": self.vehicle_id,
                "status": "maintenance_pending",
                "maintenance_type": self.maintenance_type,
                "restrictions": ["no_long_haul", "no_heavy_loads"],
                "maintenance_scheduled_date": chrono::Utc::now().to_rfc3339(),
                "current_mileage": self.current_mileage
            });

            vehicles_service
                .create(tenant_ctx.clone(), vehicle_update, params.clone())
                .await
                .map_err(|e| JobError::Retryable(format!("Failed to update vehicle status: {}", e)))?;

            // Find active routes for this vehicle
            let active_routes_query = serde_json::json!({
                "match": format!("$route isa operation, has vehicle-id '{}', has status 'active';", self.vehicle_id),
                "get": "$route;"
            });

            let active_routes = operations_service
                .custom(tenant_ctx.clone(), "read", Some(active_routes_query), params.clone())
                .await
                .map_err(|e| JobError::Retryable(format!("Failed to get active routes for vehicle {}: {}", self.vehicle_id, e)))?;

            let mut reassigned_routes = 0;

            // Reassign active routes to backup vehicles
            if let Some(routes_array) = active_routes.as_array() {
                for route in routes_array {
                    let route_id = route.get("route-id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let delivery_priority = route.get("priority").and_then(|v| v.as_str()).unwrap_or("standard");

                    // Find available backup vehicles
                    let backup_vehicles_query = serde_json::json!({
                        "match": "$vehicle isa vehicle, has status 'available', has maintenance-status 'good';",
                        "get": "$vehicle;"
                    });

                    let backup_vehicles = vehicles_service
                        .custom(tenant_ctx.clone(), "read", Some(backup_vehicles_query), params.clone())
                        .await
                        .map_err(|e| JobError::Retryable(format!("Failed to get backup vehicles: {}", e)))?;

                    if let Some(vehicles_array) = backup_vehicles.as_array() {
                        // Select best backup vehicle based on capacity and location
                        let mut best_backup: Option<String> = None;
                        let mut best_score = 0.0;

                        for backup_vehicle in vehicles_array {
                            let backup_id = backup_vehicle.get("vehicle-id").and_then(|v| v.as_str()).unwrap_or("unknown");
                            let capacity = backup_vehicle.get("capacity").and_then(|v| v.as_f64()).unwrap_or(1000.0);
                            let fuel_level = backup_vehicle.get("fuel-level").and_then(|v| v.as_f64()).unwrap_or(50.0);
                            let maintenance_score = backup_vehicle.get("maintenance-score").and_then(|v| v.as_f64()).unwrap_or(3.0);

                            // Calculate suitability score
                            let score = (capacity / 2000.0) + (fuel_level / 100.0) + (maintenance_score / 5.0);
                            
                            if score > best_score {
                                best_score = score;
                                best_backup = Some(backup_id.to_string());
                            }
                        }

                        // Reassign route to best backup vehicle
                        if let Some(backup_vehicle_id) = best_backup {
                            let reassignment_data = serde_json::json!({
                                "route_id": route_id,
                                "original_vehicle_id": self.vehicle_id,
                                "new_vehicle_id": backup_vehicle_id,
                                "reassignment_reason": format!("Maintenance required: {}", self.maintenance_type),
                                "reassignment_timestamp": chrono::Utc::now().to_rfc3339(),
                                "priority": delivery_priority
                            });

                            operations_service
                                .create(tenant_ctx.clone(), reassignment_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to reassign route {}: {}", route_id, e)))?;

                            reassigned_routes += 1;
                        }
                    }
                }
            }

            // Schedule maintenance window during low-demand periods
            let maintenance_window = self.calculate_optimal_maintenance_window(&ctx).await;
            
            let maintenance_schedule = serde_json::json!({
                "vehicle_id": self.vehicle_id,
                "maintenance_type": self.maintenance_type,
                "scheduled_start": maintenance_window.start,
                "scheduled_end": maintenance_window.end,
                "estimated_duration_hours": maintenance_window.duration,
                "maintenance_facility": get_config_value(&ctx, "maintenance.facility_name", "Central Maintenance Hub").await,
                "parts_required": self.get_required_parts(),
                "technician_required": true,
                "created_timestamp": chrono::Utc::now().to_rfc3339()
            });

            operations_service
                .create(tenant_ctx.clone(), maintenance_schedule, params.clone())
                .await
                .map_err(|e| JobError::Retryable(format!("Failed to schedule maintenance: {}", e)))?;

            // Notify maintenance team and operations
            let notification_data = serde_json::json!({
                "notification_type": "maintenance_scheduled",
                "vehicle_id": self.vehicle_id,
                "maintenance_type": self.maintenance_type,
                "scheduled_date": maintenance_window.start,
                "current_mileage": self.current_mileage,
                "routes_reassigned": reassigned_routes,
                "urgency": if self.current_mileage > maintenance_threshold + get_config_value(&ctx, "maintenance.high_priority_mileage_buffer", "5000").await.parse().unwrap_or(5000) { "high" } else { "normal" }
            });

            operations_service
                .create(tenant_ctx.clone(), notification_data, params.clone())
                .await
                .map_err(|e| JobError::Retryable(format!("Failed to create maintenance notification: {}", e)))?;

            // Notify maintenance technicians
            let technician_notification = serde_json::json!({
                "notification_type": "maintenance_assignment",
                "vehicle_id": self.vehicle_id,
                "maintenance_type": self.maintenance_type,
                "scheduled_start": maintenance_window.start,
                "scheduled_end": maintenance_window.end,
                "estimated_duration_hours": maintenance_window.duration,
                "parts_required": self.get_required_parts(),
                "priority": if self.current_mileage > maintenance_threshold + get_config_value(&ctx, "maintenance.high_priority_mileage_buffer", "5000").await.parse().unwrap_or(5000) { "high" } else { "normal" },
                "facility": get_config_value(&ctx, "maintenance.facility_name", "Central Maintenance Hub").await
            });

            employees_service
                .create(tenant_ctx.clone(), technician_notification, params.clone())
                .await
                .map_err(|e| JobError::Retryable(format!("Failed to notify maintenance technicians: {}", e)))?;

            Ok(format!("Maintenance scheduled for vehicle {}: {} routes reassigned, maintenance window: {} - {}", 
                      self.vehicle_id, reassigned_routes, maintenance_window.start, maintenance_window.end))
        } else {
            Ok(format!("Vehicle {} does not require maintenance yet ({}km < {}km)", 
                      self.vehicle_id, self.current_mileage, self.maintenance_threshold))
        }
    }
}

#[derive(Debug)]
struct MaintenanceWindow {
    start: String,
    end: String,
    duration: i32,
}

impl MaintenanceSchedulingJob {
    async fn calculate_optimal_maintenance_window(&self, ctx: &FleetContext) -> MaintenanceWindow {
        // Calculate optimal maintenance window (typically overnight or weekends)
        let now = chrono::Utc::now();
        let maintenance_duration = match self.maintenance_type.as_str() {
            "oil_change" => 2,
            "brake_service" => 4,
            "transmission_service" => 6,
            "engine_overhaul" => 12,
            _ => 4,
        };

        // Get configurable maintenance window hours - try TypeDB rules first, then app state
        let window_start_hour = get_config_value(ctx, "maintenance.window_start_hour", "22").await.parse().unwrap_or(22);
        let window_end_hour = get_config_value(ctx, "maintenance.window_end_hour", "6").await.parse().unwrap_or(6);

        // Schedule for next available overnight window
        let mut start_time = now.date_naive().and_hms_opt(window_start_hour, 0, 0).unwrap();
        if now.time() > chrono::NaiveTime::from_hms_opt(window_start_hour, 0, 0).unwrap() {
            start_time = start_time + chrono::Duration::days(1);
        }

        // Calculate end time based on maintenance duration, but ensure it doesn't exceed window end
        let calculated_end_time = start_time + chrono::Duration::hours(maintenance_duration as i64);
        let window_end_time = if window_end_hour < window_start_hour {
            // Window spans midnight (e.g., 22:00 to 06:00 next day)
            start_time.date() + chrono::Duration::days(1)
        } else {
            // Window is within same day
            start_time.date()
        }.and_hms_opt(window_end_hour, 0, 0).unwrap();

        let end_time = calculated_end_time.min(window_end_time);

        MaintenanceWindow {
            start: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(start_time, chrono::Utc).to_rfc3339(),
            end: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(end_time, chrono::Utc).to_rfc3339(),
            duration: maintenance_duration,
        }
    }

    fn get_required_parts(&self) -> Vec<String> {
        match self.maintenance_type.as_str() {
            "oil_change" => vec!["engine_oil".to_string(), "oil_filter".to_string()],
            "brake_service" => vec!["brake_pads".to_string(), "brake_fluid".to_string()],
            "transmission_service" => vec!["transmission_fluid".to_string(), "transmission_filter".to_string()],
            "major_service" => vec!["engine_oil".to_string(), "oil_filter".to_string(), "air_filter".to_string(), "spark_plugs".to_string()],
            _ => vec!["general_parts".to_string()],
        }
    }
}
