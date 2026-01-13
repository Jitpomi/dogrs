use anyhow::Result;
use dog_core::{DogApp, tenant::TenantContext};
use serde_json::{json, Value};
use std::env;
use crate::services::FleetParams;

/// Configure all application settings including external APIs and business rules
pub fn config(dog_app: &DogApp<Value, FleetParams>) -> Result<()> {
    // HTTP Server Configuration
    configure_http(dog_app)?;
    
    // External API Configuration
    configure_external_apis(dog_app)?;
    
    // Business Rules Configuration
    configure_business_rules(dog_app);
    
    Ok(())
}

/// Get configuration value with priority: TypeDB rules > env vars > defaults
pub async fn get_config_value(dog_app: &DogApp<Value, FleetParams>, key: &str, default: &str) -> String {
    // 1. Try to get from TypeDB rules (highest priority)
    if let Ok(rules_service) = dog_app.service("rules") {
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
    
    // 2. Try environment variables
    if let Ok(env_value) = env::var(key.to_uppercase().replace(".", "_")) {
        return env_value;
    }
    
    // 3. Use default value
    default.to_string()
}

/// Configure HTTP server settings
fn configure_http(dog_app: &DogApp<Value, FleetParams>) -> Result<()> {
    let host = env::var("HTTP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("HTTP_PORT").unwrap_or_else(|_| "3036".to_string());
    
    dog_app.set("http.host", host);
    dog_app.set("http.port", port);
    Ok(())
}

/// Configure external API integrations
fn configure_external_apis(dog_app: &DogApp<Value, FleetParams>) -> Result<()> {
    // TomTom API configuration from environment variables
    let tomtom_key = env::var("TOMTOM_API_KEY")?;
    let tomtom_base_url = env::var("TOMTOM_BASE_URL")?;
    
    dog_app.set("tomtom.key", tomtom_key);
    dog_app.set("tomtom.baseUrl", tomtom_base_url);
    
    Ok(())
}

/// Configure all business rule parameters
fn configure_business_rules(dog_app: &DogApp<Value, FleetParams>) {
    configure_employee_scoring(dog_app);
    configure_sla_thresholds(dog_app);
    configure_route_rebalancing(dog_app);
    configure_maintenance(dog_app);
    configure_tracking(dog_app);
}

/// Configure employee assignment scoring algorithm
fn configure_employee_scoring(dog_app: &DogApp<Value, FleetParams>) {
    let proximity_weight = env::var("EMPLOYEE_SCORING_PROXIMITY_WEIGHT").unwrap_or_else(|_| "0.3".to_string());
    let availability_weight = env::var("EMPLOYEE_SCORING_AVAILABILITY_WEIGHT").unwrap_or_else(|_| "0.3".to_string());
    let performance_weight = env::var("EMPLOYEE_SCORING_PERFORMANCE_WEIGHT").unwrap_or_else(|_| "0.2".to_string());
    let certification_weight = env::var("EMPLOYEE_SCORING_CERTIFICATION_WEIGHT").unwrap_or_else(|_| "0.2".to_string());
    let max_daily_hours = env::var("EMPLOYEE_SCORING_MAX_DAILY_HOURS").unwrap_or_else(|_| "11.0".to_string());
    let max_performance_rating = env::var("EMPLOYEE_SCORING_MAX_PERFORMANCE_RATING").unwrap_or_else(|_| "5.0".to_string());
    
    dog_app.set("employee.scoring.proximity_weight", proximity_weight);
    dog_app.set("employee.scoring.availability_weight", availability_weight);
    dog_app.set("employee.scoring.performance_weight", performance_weight);
    dog_app.set("employee.scoring.certification_weight", certification_weight);
    dog_app.set("employee.scoring.max_daily_hours", max_daily_hours);
    dog_app.set("employee.scoring.max_performance_rating", max_performance_rating);
}

/// Configure SLA monitoring thresholds per customer tier
fn configure_sla_thresholds(dog_app: &DogApp<Value, FleetParams>) {
    // Premium customer SLA thresholds
    let premium_escalation = env::var("SLA_PREMIUM_ESCALATION_THRESHOLD_MINUTES").unwrap_or_else(|_| "15".to_string());
    let premium_notification = env::var("SLA_PREMIUM_NOTIFICATION_THRESHOLD_MINUTES").unwrap_or_else(|_| "5".to_string());
    
    // Priority customer SLA thresholds
    let priority_escalation = env::var("SLA_PRIORITY_ESCALATION_THRESHOLD_MINUTES").unwrap_or_else(|_| "30".to_string());
    let priority_notification = env::var("SLA_PRIORITY_NOTIFICATION_THRESHOLD_MINUTES").unwrap_or_else(|_| "15".to_string());
    
    // Standard customer SLA thresholds
    let standard_escalation = env::var("SLA_STANDARD_ESCALATION_THRESHOLD_MINUTES").unwrap_or_else(|_| "60".to_string());
    let standard_notification = env::var("SLA_STANDARD_NOTIFICATION_THRESHOLD_MINUTES").unwrap_or_else(|_| "30".to_string());
    
    dog_app.set("sla.premium.escalation_threshold_minutes", premium_escalation);
    dog_app.set("sla.premium.notification_threshold_minutes", premium_notification);
    dog_app.set("sla.priority.escalation_threshold_minutes", priority_escalation);
    dog_app.set("sla.priority.notification_threshold_minutes", priority_notification);
    dog_app.set("sla.standard.escalation_threshold_minutes", standard_escalation);
    dog_app.set("sla.standard.notification_threshold_minutes", standard_notification);
}

/// Configure route rebalancing parameters
fn configure_route_rebalancing(dog_app: &DogApp<Value, FleetParams>) {
    let traffic_delay_trigger = env::var("REBALANCING_TRAFFIC_DELAY_TRIGGER_MINUTES").unwrap_or_else(|_| "30".to_string());
    let premium_threshold = env::var("REBALANCING_PREMIUM_THRESHOLD_SECONDS").unwrap_or_else(|_| "7200".to_string());
    let priority_threshold = env::var("REBALANCING_PRIORITY_THRESHOLD_SECONDS").unwrap_or_else(|_| "10800".to_string());
    let standard_threshold = env::var("REBALANCING_STANDARD_THRESHOLD_SECONDS").unwrap_or_else(|_| "14400".to_string());
    
    dog_app.set("rebalancing.traffic_delay_trigger_minutes", traffic_delay_trigger);
    dog_app.set("rebalancing.premium_threshold_seconds", premium_threshold);
    dog_app.set("rebalancing.priority_threshold_seconds", priority_threshold);
    dog_app.set("rebalancing.standard_threshold_seconds", standard_threshold);
}

/// Configure maintenance scheduling parameters
fn configure_maintenance(dog_app: &DogApp<Value, FleetParams>) {
    let mileage_threshold = env::var("MAINTENANCE_DEFAULT_MILEAGE_THRESHOLD").unwrap_or_else(|_| "50000".to_string());
    let window_start_hour = env::var("MAINTENANCE_WINDOW_START_HOUR").unwrap_or_else(|_| "22".to_string());
    let window_end_hour = env::var("MAINTENANCE_WINDOW_END_HOUR").unwrap_or_else(|_| "6".to_string());
    
    dog_app.set("maintenance.default_mileage_threshold", mileage_threshold);
    dog_app.set("maintenance.window_start_hour", window_start_hour);
    dog_app.set("maintenance.window_end_hour", window_end_hour);
}

/// Configure GPS tracking and monitoring parameters
fn configure_tracking(dog_app: &DogApp<Value, FleetParams>) {
    let premium_interval = env::var("TRACKING_PREMIUM_UPDATE_INTERVAL_SECONDS").unwrap_or_else(|_| "300".to_string());
    let priority_interval = env::var("TRACKING_PRIORITY_UPDATE_INTERVAL_SECONDS").unwrap_or_else(|_| "600".to_string());
    let standard_interval = env::var("TRACKING_STANDARD_UPDATE_INTERVAL_SECONDS").unwrap_or_else(|_| "1800".to_string());
    let certificate_warning_days = env::var("TRACKING_CERTIFICATE_WARNING_DAYS").unwrap_or_else(|_| "30".to_string());
    
    dog_app.set("tracking.premium_update_interval_seconds", premium_interval);
    dog_app.set("tracking.priority_update_interval_seconds", priority_interval);
    dog_app.set("tracking.standard_update_interval_seconds", standard_interval);
    dog_app.set("tracking.certificate_warning_days", certificate_warning_days);
}
