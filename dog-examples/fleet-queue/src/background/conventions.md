# Background Processing Conventions

## Overview
This document establishes clear conventions for background job processing in the fleet management system using dog-queue with TypeDB-based dynamic configuration.

## Architecture Principles

### 1. Job Definition Pattern
```rust
// Standard job structure with dog-queue
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
pub struct JobName {
    // Job-specific data fields
    pub field: String,
    pub priority: String,
    pub created_at: String,
}

#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

#[async_trait]
impl Job for JobName {
    type Context = FleetContext;
    type Result = String;
    
    const JOB_TYPE: &'static str = "job_name";
    
    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        // Get configurable parameters using TypeDB rules
        let threshold = get_config_value(&ctx, "job.threshold", "100").await.parse().unwrap_or(100);
        
        // Implementation with dynamic configuration
        Ok(format!("Job completed with threshold: {}", threshold))
    }
}
```

### 2. Background System Setup Pattern
```rust
// Current implementation using dog-queue with memory backend
use dog_queue::{QueueAdapter, MemoryBackend, WorkerHandle};
use std::sync::Arc;
use tokio::time::{interval, Duration};

pub struct BackgroundSystem {
    adapter: Arc<QueueAdapter<MemoryBackend>>,
    worker_handles: Vec<WorkerHandle>,
    context: GPSFleetContext,
}

impl BackgroundSystem {
    pub async fn new(app: Arc<AxumApp<Value, FleetParams>>) -> Result<Self> {
        let backend = MemoryBackend::new();
        
        // Create queue adapter with proper configuration
        let config = QueueConfig {
            max_workers: 10,
            worker_idle_timeout: Duration::from_secs(60),
            lease_duration: Duration::from_secs(300),
            heartbeat_interval: Duration::from_secs(30),
            max_retry_backoff: Duration::from_secs(3600),
            base_retry_backoff: Duration::from_secs(1),
        };
        
        let adapter = Arc::new(QueueAdapter::with_config(backend, config));
        
        // Register implemented job types only
        adapter.register_job::<GPSTrackingJob>().await?;
        
        let context = GPSFleetContext { app };
        
        Ok(Self {
            adapter,
            worker_handles: Vec::new(),
            context,
        })
    }
    
    pub async fn start(&mut self) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        
        // Start workers for implemented job types only
        let queues = vec![
            "gps_tracking".to_string(),
        ];
        
        let worker_handle = self.adapter.start_workers(
            ctx.clone(),
            self.context.clone(),
            queues,
        ).await?;
        
        self.worker_handles.push(worker_handle);
        
        // Start cron jobs
        self.start_cron_jobs().await;
        
        Ok(())
    }
    
    async fn start_cron_jobs(&self) {
        let adapter = Arc::clone(&self.adapter);
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        
        // GPS tracking every minute
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60)); // Every minute
            loop {
                interval.tick().await;
                // Query for active assignments and enqueue GPS tracking jobs
                // See actual implementation in BackgroundSystem::start_cron_jobs()
            }
        });
    }
}
```

## Implemented Job Types

### 1. GPSTrackingJob (Currently Implemented)
- **Purpose**: Periodic location tracking and status updates
- **Frequency**: Every minute (60 seconds)
- **Queue**: `gps_tracking`
- **Context**: Fleet assignment monitoring
- **Status**: ✅ Registered and active in BackgroundSystem

### Additional Job Types (Implemented but not registered in BackgroundSystem)

### 2. EmployeeAssignmentJob
- **Purpose**: Intelligent employee assignment based on scoring algorithm
- **Triggers**: Manual enqueue via REST API or service calls
- **Configuration**: Dynamic scoring weights via TypeDB rules
  - `employee.scoring.proximity_weight`
  - `employee.scoring.availability_weight`
  - `employee.scoring.performance_weight`
  - `employee.scoring.certification_weight`
- **Status**: ✅ Implemented and registered in BackgroundSystem

### 3. RouteRebalancingJob
- **Purpose**: Dynamic route optimization based on traffic and delays
- **Triggers**: Manual enqueue via REST API or service calls
- **Configuration**: Configurable thresholds via TypeDB rules
  - `rebalancing.traffic_delay_trigger_minutes`
  - `rebalancing.premium_threshold_seconds`
  - `rebalancing.priority_threshold_seconds`
- **Status**: ⚠️ Implemented but not registered in BackgroundSystem

### 4. SLAMonitoringJob
- **Purpose**: Customer SLA compliance monitoring and breach prevention
- **Triggers**: Manual enqueue via REST API or service calls
- **Configuration**: Tier-specific thresholds via TypeDB rules
  - `sla.premium.escalation_threshold_minutes`
  - `sla.priority.escalation_threshold_minutes`
  - `sla.standard.escalation_threshold_minutes`
- **Status**: ⚠️ Implemented but not registered in BackgroundSystem

### 5. MaintenanceSchedulingJob
- **Purpose**: Predictive vehicle maintenance scheduling
- **Triggers**: Manual enqueue via REST API or service calls
- **Configuration**: Configurable maintenance windows via TypeDB rules
  - `maintenance.default_mileage_threshold`
  - `maintenance.window_start_hour`
  - `maintenance.window_end_hour`
- **Status**: ⚠️ Implemented but not registered in BackgroundSystem

### 6. ComplianceMonitoringJob
- **Purpose**: Regulatory compliance automation (DOT HOS, CDL, medical certs)
- **Triggers**: Manual enqueue via REST API or service calls
- **Configuration**: Configurable compliance limits via TypeDB rules
  - `compliance.daily_driving_limit_hours`
  - `compliance.consecutive_duty_limit_hours`
  - `compliance.cdl_warning_days`
- **Status**: ⚠️ Implemented but not registered in BackgroundSystem

## Dynamic Configuration System

### 1. TypeDB Rules-Based Configuration
```rust
// Configuration priority: TypeDB rules > environment variables > defaults
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
```

### 2. Scheduled Rule Changes
```javascript
// Example: Schedule rule changes via REST API
POST /rules
{
  "rule_name": "sla.premium.escalation_threshold_minutes",
  "rule_value": "10",
  "rule_category": "sla_monitoring",
  "effective_from": "2026-01-15T08:00:00Z",
  "effective_until": "2026-01-15T18:00:00Z",
  "status": "active"
}
```

### 3. Configuration Categories
- **Employee Scoring**: `employee.scoring.*`
- **SLA Thresholds**: `sla.{tier}.*`
- **Route Rebalancing**: `rebalancing.*`
- **Maintenance**: `maintenance.*`
- **Compliance**: `compliance.*`
- **Tracking**: `tracking.*`

## Error Handling Conventions

### 1. Job Error Types
```rust
// Use JobError from dog-queue
return Err(JobError::Permanent(format!("Invalid input: {}", error)));
return Err(JobError::Retryable(format!("API call failed: {}", error)));
```

### 2. Service Integration Errors
- **Permanent errors**: Invalid data, missing required fields, authorization failures
- **Retryable errors**: Network timeouts, temporary service unavailability, rate limits
- **Configuration errors**: Missing rules fall back to app state, then defaults

## Service Integration Patterns

### 1. Accessing Services
```rust
// Standard service access pattern
let operations_service = ctx.app.app.service("operations")
    .map_err(|e| JobError::Permanent(format!("Operations service not found: {}", e)))?;

let tomtom_service = ctx.app.app.service("tomtom")
    .map_err(|e| JobError::Permanent(format!("TomTom service not found: {}", e)))?;
```

### 2. TypeDB Queries
```rust
// Standard TypeDB query pattern
let query = serde_json::json!({
    "match": format!("$entity isa entity_type, has entity-id '{}';", id),
    "get": "$entity;"
});

let result = service
    .custom(tenant_ctx.clone(), "read", Some(query), params.clone())
    .await
    .map_err(|e| JobError::Retryable(format!("Failed to query entity: {}", e)))?;
```

### 3. TomTom API Integration
```rust
// Standard TomTom service calls
let eta_result = tomtom_service
    .custom(tenant_ctx.clone(), "eta", Some(serde_json::json!({
        "vehicle_id": vehicle_id,
        "current_lat": current_lat,
        "current_lng": current_lng,
        "dest_lat": dest_lat,
        "dest_lng": dest_lng
    })), params.clone())
    .await
    .map_err(|e| JobError::Retryable(format!("Failed to calculate ETA: {}", e)))?;
```
