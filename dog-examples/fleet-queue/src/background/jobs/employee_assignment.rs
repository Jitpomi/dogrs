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
pub struct EmployeeAssignmentJob {
    pub route_id: String,
    pub pickup_location: (f64, f64),
    pub delivery_priority: String,
    pub required_certifications: Vec<String>,
}

impl EmployeeAssignmentJob {
    pub fn new(route_id: String, pickup_location: (f64, f64), delivery_priority: String, required_certifications: Vec<String>) -> Self {
        Self { 
            route_id, 
            pickup_location, 
            delivery_priority, 
            required_certifications 
        }
    }
}

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for EmployeeAssignmentJob {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "employee_assignment";
    const PRIORITY: JobPriority = JobPriority::High;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let tenant_ctx = TenantContext::new("fleet_tenant".to_string());
        let params = FleetParams::default();
        
        let employees_service = ctx.app.app.service("employees")
            .map_err(|e| JobError::Permanent(format!("Employees service not found: {}", e)))?;
        
        let tomtom_service = ctx.app.app.service("tomtom")
            .map_err(|e| JobError::Permanent(format!("TomTom service not found: {}", e)))?;
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Get all available employees with driver role
        let employees_query = serde_json::json!({
            "match": "$employee isa employee, has employee-role 'driver', has status 'available';",
            "select": "$employee;"
        });
        
        let available_employees = employees_service
            .custom(tenant_ctx.clone(), "read", Some(employees_query), params.clone())
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to get available employees: {}", e)))?;

        let mut scored_employees = Vec::new();

        // Score each employee based on multiple criteria
        if let Some(employees_array) = available_employees.as_array() {
            for employee in employees_array {
                let employee_id = employee.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                let current_lat = employee.get("current-lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let current_lng = employee.get("current-lng").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let hours_worked = employee.get("daily-hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let performance_rating = employee.get("performance-rating").and_then(|v| v.as_f64()).unwrap_or(3.0);
                let empty_vec = vec![];
                let certifications = employee.get("certifications").and_then(|v| v.as_array()).unwrap_or(&empty_vec);

                // Calculate proximity score using TomTom
                let distance_result = tomtom_service
                    .custom(tenant_ctx.clone(), "route", Some(serde_json::json!({
                        "from_lat": current_lat,
                        "from_lng": current_lng,
                        "to_lat": self.pickup_location.0,
                        "to_lng": self.pickup_location.1
                    })), params.clone())
                    .await
                    .map_err(|e| JobError::Retryable(format!("Failed to calculate distance for employee {}: {}", employee_id, e)))?;

                let distance_meters = distance_result.get("distance_meters").and_then(|v| v.as_i64()).unwrap_or(get_config_value(&ctx, "employee.scoring.max_distance_fallback", "999999").await.parse().unwrap_or(999999)) as f64;
                let travel_time = distance_result.get("duration_seconds").and_then(|v| v.as_i64()).unwrap_or(get_config_value(&ctx, "employee.scoring.max_travel_time_fallback", "3600").await.parse().unwrap_or(3600)) as f64;

                // Get configurable scoring weights - try TypeDB rules first, then app state
                let proximity_weight: f64 = get_config_value(&ctx, "employee.scoring.proximity_weight", "0.3").await.parse().unwrap_or(0.3);
                let availability_weight: f64 = get_config_value(&ctx, "employee.scoring.availability_weight", "0.3").await.parse().unwrap_or(0.3);
                let performance_weight: f64 = get_config_value(&ctx, "employee.scoring.performance_weight", "0.2").await.parse().unwrap_or(0.2);
                let certification_weight: f64 = get_config_value(&ctx, "employee.scoring.certification_weight", "0.2").await.parse().unwrap_or(0.2);
                let max_daily_hours: f64 = get_config_value(&ctx, "employee.scoring.max_daily_hours", "11.0").await.parse().unwrap_or(11.0);
                let max_performance_rating: f64 = get_config_value(&ctx, "employee.scoring.max_performance_rating", "5.0").await.parse().unwrap_or(5.0);

                // Calculate composite score with configurable weights
                let distance_divisor = get_config_value(&ctx, "employee.scoring.distance_divisor", "1000.0").await.parse().unwrap_or(1000.0);
                let proximity_score = 1.0 / (1.0 + distance_meters / distance_divisor); // Closer = higher score
                let availability_score = (max_daily_hours - hours_worked) / max_daily_hours; // Less hours worked = higher score
                let performance_score = performance_rating / max_performance_rating; // Normalize to 0-1
                let certification_score = if self.required_certifications.is_empty() { 1.0 } else {
                    let has_required = self.required_certifications.iter().all(|req| {
                        certifications.iter().any(|cert| cert.as_str() == Some(req))
                    });
                    if has_required { 1.0 } else { 0.0 }
                };

                let total_score = (proximity_score * proximity_weight) + (availability_score * availability_weight) + 
                                (performance_score * performance_weight) + (certification_score * certification_weight);

                scored_employees.push((employee_id.to_string(), total_score, travel_time));
            }
        }

        // Sort by score (highest first)
        scored_employees.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        if let Some((best_employee_id, score, eta)) = scored_employees.first() {
            // Assign the route to the best employee
            let assignment_data = serde_json::json!({
                "route_id": self.route_id,
                "employee_id": best_employee_id,
                "assignment_score": score,
                "estimated_pickup_time": eta,
                "assignment_timestamp": chrono::Utc::now().to_rfc3339(),
                "assignment_type": "intelligent_assignment"
            });

            operations_service
                .create(tenant_ctx, assignment_data, params)
                .await
                .map_err(|e| JobError::Retryable(format!("Failed to create assignment: {}", e)))?;

            Ok(format!("Assigned route {} to employee {} with score {:.2}", 
                      self.route_id, best_employee_id, score))
        } else {
            Err(JobError::Retryable("No suitable employees found for assignment".to_string()))
        }
    }
}
