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
        
        let employees_service = ctx.app.app.service("employees")
            .map_err(|e| JobError::Permanent(format!("Employees service not found: {}", e)))?;
        
        let operations_service = ctx.app.app.service("operations")
            .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

        // Get employee details and current status
        let employee_query = serde_json::json!({
            "query": format!("match $employee isa employee, has id '{}'; select $employee;", self.employee_id)
        });

        let employee_result = employees_service
            .custom(tenant_ctx.clone(), "read", Some(employee_query), params.clone())
            .await
            .map_err(|e| JobError::Retryable(format!("Failed to get driver details: {}", e)))?;

        if let Some(employee_array) = employee_result.as_array() {
            if let Some(employee) = employee_array.first() {
                let daily_drive_hours = employee.get("daily-drive-hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let consecutive_hours = employee.get("consecutive-hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let default_rest_hours = get_config_value(&ctx, "compliance.default_rest_hours_fallback", "10.0").await.parse().unwrap_or(10.0);
                let rest_hours = employee.get("rest-hours").and_then(|v| v.as_f64()).unwrap_or(default_rest_hours);
                let last_break_time = employee.get("last-break-time").and_then(|v| v.as_str()).unwrap_or("");
                let cdl_expiry = employee.get("cdl-expiry").and_then(|v| v.as_str()).unwrap_or("");
                let medical_cert_expiry = employee.get("medical-cert-expiry").and_then(|v| v.as_str()).unwrap_or("");

                let mut violations = Vec::new();
                let mut actions_taken = Vec::new();

                // DOT Hours of Service (HOS) Compliance Checks
                
                // Get configurable compliance thresholds - try TypeDB rules first, then app state
                let daily_driving_limit: f64 = get_config_value(&ctx, "compliance.daily_driving_limit_hours", "11.0").await.parse().unwrap_or(11.0);
                
                // 1. Daily driving limit (configurable, default 11 hours)
                if daily_drive_hours >= daily_driving_limit {
                    violations.push("daily_driving_limit_exceeded".to_string());
                    
                    // Block driver from new assignments
                    let driver_update = serde_json::json!({
                        "employee_id": self.employee_id,
                        "status": "hours_exceeded",
                        "available": false,
                        "violation_type": "daily_driving_limit",
                        "violation_timestamp": chrono::Utc::now().to_rfc3339(),
                        "required_rest_hours": get_config_value(&ctx, "compliance.required_rest_hours", "10.0").await.parse().unwrap_or(10.0)
                    });

                    employees_service
                        .create(tenant_ctx.clone(), driver_update, params.clone())
                        .await
                        .map_err(|e| JobError::Retryable(format!("Failed to update driver status: {}", e)))?;

                    actions_taken.push("driver_blocked_from_assignments".to_string());
                }

                let consecutive_duty_limit: f64 = get_config_value(&ctx, "compliance.consecutive_duty_limit_hours", "14.0").await.parse().unwrap_or(14.0);
                let mandatory_rest_hours: f64 = get_config_value(&ctx, "compliance.mandatory_rest_hours", "10.0").await.parse().unwrap_or(10.0);
                
                // 2. 14-hour consecutive duty limit (configurable)
                if consecutive_hours >= consecutive_duty_limit && rest_hours < mandatory_rest_hours {
                    violations.push("consecutive_duty_limit_exceeded".to_string());
                    
                    // Enforce mandatory 10-hour rest period
                    let mandatory_rest_end = chrono::Utc::now() + chrono::Duration::hours(10);
                    
                    let rest_enforcement = serde_json::json!({
                        "employee_id": self.employee_id,
                        "rest_type": "mandatory_10_hour",
                        "rest_start": chrono::Utc::now().to_rfc3339(),
                        "rest_end": mandatory_rest_end.to_rfc3339(),
                        "violation_reason": "14-hour consecutive duty limit exceeded",
                        "compliance_rule": "DOT_HOS_395.8"
                    });

                    operations_service
                        .create(tenant_ctx.clone(), rest_enforcement, params.clone())
                        .await
                        .map_err(|e| JobError::Retryable(format!("Failed to enforce mandatory rest: {}", e)))?;

                    actions_taken.push("mandatory_rest_enforced".to_string());
                }

                let break_requirement_hours: f64 = get_config_value(&ctx, "compliance.break_requirement_hours", "8.0").await.parse().unwrap_or(8.0);
                
                // 3. 30-minute break requirement (after configurable hours of driving)
                if daily_drive_hours >= break_requirement_hours && !last_break_time.is_empty() {
                    if let Ok(last_break) = chrono::DateTime::parse_from_rfc3339(last_break_time) {
                        let hours_since_break = (chrono::Utc::now() - last_break.with_timezone(&chrono::Utc)).num_hours();
                        let break_hours_threshold = get_config_value(&ctx, "compliance.break_hours_threshold", "8").await.parse().unwrap_or(8);
                        if hours_since_break >= break_hours_threshold {
                            violations.push("break_requirement_violation".to_string());
                            
                            let break_enforcement = serde_json::json!({
                                "employee_id": self.employee_id,
                                "break_type": get_config_value(&ctx, "compliance.mandatory_break_type", "mandatory_30_minute").await,
                                "break_required": true,
                                "hours_since_last_break": hours_since_break,
                                "compliance_rule": "DOT_HOS_395.8_break"
                            });

                            operations_service
                                .create(tenant_ctx.clone(), break_enforcement, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to enforce break requirement: {}", e)))?;

                            actions_taken.push("break_requirement_enforced".to_string());
                        }
                    }
                }

                // 4. CDL License Expiry Check
                if !cdl_expiry.is_empty() {
                    if let Ok(expiry_date) = chrono::DateTime::parse_from_rfc3339(cdl_expiry) {
                        let days_until_expiry = (expiry_date.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_days();
                        
                        if days_until_expiry <= 0 {
                            violations.push("cdl_expired".to_string());
                            
                            // Immediately suspend driver
                            let suspension_data = serde_json::json!({
                                "employee_id": self.employee_id,
                                "suspension_type": "cdl_expired",
                                "suspension_start": chrono::Utc::now().to_rfc3339(),
                                "compliance_rule": "CDL_validity_requirement",
                                "action_required": "renew_cdl_immediately"
                            });

                            employees_service
                                .create(tenant_ctx.clone(), suspension_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to suspend driver for expired CDL: {}", e)))?;

                            actions_taken.push("driver_suspended_cdl_expired".to_string());
                        } else if days_until_expiry <= get_config_value(&ctx, "compliance.cdl_warning_days", "30").await.parse().unwrap_or(30) {
                            // Warning for upcoming expiry
                            let warning_data = serde_json::json!({
                                "employee_id": self.employee_id,
                                "warning_type": "cdl_expiring_soon",
                                "days_until_expiry": days_until_expiry,
                                "expiry_date": cdl_expiry,
                                "action_required": "schedule_cdl_renewal"
                            });

                            operations_service
                                .create(tenant_ctx.clone(), warning_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to create CDL expiry warning: {}", e)))?;

                            actions_taken.push("cdl_expiry_warning_issued".to_string());
                        }
                    }
                }

                // 5. Medical Certificate Expiry Check
                if !medical_cert_expiry.is_empty() {
                    if let Ok(expiry_date) = chrono::DateTime::parse_from_rfc3339(medical_cert_expiry) {
                        let days_until_expiry = (expiry_date.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_days();
                        
                        if days_until_expiry <= 0 {
                            violations.push("medical_cert_expired".to_string());
                            
                            let suspension_data = serde_json::json!({
                                "employee_id": self.employee_id,
                                "suspension_type": "medical_cert_expired",
                                "suspension_start": chrono::Utc::now().to_rfc3339(),
                                "compliance_rule": "DOT_medical_certificate_requirement",
                                "action_required": "renew_medical_certificate"
                            });

                            employees_service
                                .create(tenant_ctx.clone(), suspension_data, params.clone())
                                .await
                                .map_err(|e| JobError::Retryable(format!("Failed to suspend driver for expired medical cert: {}", e)))?;

                            actions_taken.push("driver_suspended_medical_cert_expired".to_string());
                        }
                    }
                }

                // Generate compliance report
                let compliance_report = serde_json::json!({
                    "employee_id": self.employee_id,
                    "monitoring_type": self.monitoring_type,
                    "shift_start_time": self.shift_start_time,
                    "daily_drive_hours": daily_drive_hours,
                    "consecutive_hours": consecutive_hours,
                    "rest_hours": rest_hours,
                    "violations": violations,
                    "actions_taken": actions_taken,
                    "compliance_status": if violations.is_empty() { "compliant" } else { "violations_detected" },
                    "monitoring_timestamp": chrono::Utc::now().to_rfc3339(),
                    "next_monitoring_required": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339()
                });

                operations_service
                    .create(tenant_ctx, compliance_report, params)
                    .await
                    .map_err(|e| JobError::Retryable(format!("Failed to create compliance report: {}", e)))?;

                if violations.is_empty() {
                    Ok(format!("Compliance monitoring completed for driver {}: No violations detected", self.employee_id))
                } else {
                    Ok(format!("Compliance monitoring completed for driver {}: {} violations detected, {} actions taken", 
                              self.employee_id, violations.len(), actions_taken.len()))
                }
            } else {
                Err(JobError::Permanent(format!("Driver {} not found", self.employee_id)))
            }
        } else {
            Err(JobError::Permanent(format!("Invalid driver query result for {}", self.employee_id)))
        }
    }
}
