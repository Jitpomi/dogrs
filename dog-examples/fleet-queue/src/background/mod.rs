/// Production-ready background processing system for fleet operations
/// 
/// This module provides proper dog-queue integration following the actual API patterns.

pub mod jobs;

use anyhow::Result;
use dog_queue::prelude::*;
use dog_queue::backend::memory::MemoryBackend;
use dog_queue::{WorkerHandle, QueueConfig};
use serde_json::Value;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use dog_axum::AxumApp;
use crate::services::FleetParams;

pub use jobs::*;

/// Unified context for all background jobs
#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
}

/// Main background processing system using proper dog-queue patterns
pub struct BackgroundSystem {
    adapter: Arc<QueueAdapter<MemoryBackend>>,
    worker_handles: Vec<WorkerHandle>,
    context: FleetContext,
}

impl BackgroundSystem {
    /// Create new background system with proper dog-queue integration
    pub async fn new(app: Arc<AxumApp<Value, FleetParams>>) -> Result<Self> {
        // Create memory backend for now (can be swapped for Redis/PostgreSQL)
        let backend = MemoryBackend::new();
        
        // Create queue adapter with configurable settings
        let max_workers = std::env::var("QUEUE_MAX_WORKERS").unwrap_or_else(|_| "10".to_string()).parse().unwrap_or(10);
        let worker_idle_timeout = std::env::var("QUEUE_WORKER_IDLE_TIMEOUT_SECS").unwrap_or_else(|_| "60".to_string()).parse().unwrap_or(60);
        let lease_duration = std::env::var("QUEUE_LEASE_DURATION_SECS").unwrap_or_else(|_| "300".to_string()).parse().unwrap_or(300);
        let heartbeat_interval = std::env::var("QUEUE_HEARTBEAT_INTERVAL_SECS").unwrap_or_else(|_| "30".to_string()).parse().unwrap_or(30);
        let max_retry_backoff = std::env::var("QUEUE_MAX_RETRY_BACKOFF_SECS").unwrap_or_else(|_| "3600".to_string()).parse().unwrap_or(3600);
        let base_retry_backoff = std::env::var("QUEUE_BASE_RETRY_BACKOFF_SECS").unwrap_or_else(|_| "1".to_string()).parse().unwrap_or(1);
        
        let config = QueueConfig {
            max_workers,
            worker_idle_timeout: Duration::from_secs(worker_idle_timeout),
            lease_duration: Duration::from_secs(lease_duration),
            heartbeat_interval: Duration::from_secs(heartbeat_interval),
            max_retry_backoff: Duration::from_secs(max_retry_backoff),
            base_retry_backoff: Duration::from_secs(base_retry_backoff),
        };
        
        let adapter = Arc::new(QueueAdapter::with_config(backend, config));
        
        // Register all implemented job types
        adapter.register_job::<GPSTrackingJob>().await?;
        adapter.register_job::<EmployeeAssignmentJob>().await?;
        adapter.register_job::<RouteRebalancingJob>().await?;
        adapter.register_job::<SLAMonitoringJob>().await?;
        adapter.register_job::<MaintenanceSchedulingJob>().await?;
        adapter.register_job::<ComplianceMonitoringJob>().await?;
        
        let context = FleetContext {
            app,
        };
        
        Ok(Self {
            adapter,
            worker_handles: Vec::new(),
            context,
        })
    }
    
    /// Start background processing workers
    pub async fn start(&mut self) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        
        // Start workers for all implemented job types
        let queues = vec![
            "gps_tracking".to_string(),
            "employee_assignment".to_string(),
            "route_rebalancing".to_string(),
            "sla_monitoring".to_string(),
            "maintenance_scheduling".to_string(),
            "compliance_monitoring".to_string(),
        ];
        
        println!("🔧 Starting dog-queue workers for queues: {:?}", queues);
        let worker_handle = self.adapter.start_workers(
            ctx.clone(),
            self.context.clone(),
            queues,
        ).await?;
        
        self.worker_handles.push(worker_handle);
        
        // Start cron jobs
        self.start_cron_jobs().await;
        
        println!("🚀 Background processing system started with proper dog-queue integration");
        println!("📊 Workers active: {}", self.worker_handles.len());
        Ok(())
    }
    
    /// Start periodic cron jobs
    async fn start_cron_jobs(&self) {
        let adapter = Arc::clone(&self.adapter);
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        
        // GPS tracking every minute
        {
            let adapter = Arc::clone(&adapter);
            let ctx = ctx.clone();
            let context = self.context.clone();
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    
                    // Query operations service for active assignments
                    if let Ok(operations_service) = context.app.app.service("operations") {
                        let tenant_ctx = dog_core::tenant::TenantContext::new("fleet_tenant".to_string());
                        let params = crate::services::FleetParams::default();
                        
                        // Query for active assignments
                        let query = serde_json::json!({
                            "query": "match $assignment isa operation, has status \"active\", has operation-id $id; select $assignment, $id;"
                        });
                        
                        println!("🔄 Cron job: Querying for active assignments...");
                        if let Ok(assignments_result) = operations_service.custom(tenant_ctx, "read", Some(query), params).await {
                            println!("✅ Cron job: Found assignments, processing...");
                            if let Some(ok_result) = assignments_result.get("ok") {
                                if let Some(answers) = ok_result.get("answers").and_then(|v| v.as_array()) {
                                    println!("📊 Cron job: Processing {} assignments", answers.len());
                                    for assignment in answers {
                                        if let Some(data) = assignment.get("data") {
                                            if let Some(id_attr) = data.get("id") {
                                                if let Some(assignment_id) = id_attr.get("value").and_then(|v| v.as_str()) {
                                                    println!("🎯 Cron job: Enqueuing GPS job for assignment {}", assignment_id);
                                                    let job = crate::background::jobs::GPSTrackingJob::new(assignment_id.to_string());
                                                    if let Err(e) = adapter.enqueue(ctx.clone(), job).await {
                                                        eprintln!("❌ Failed to enqueue GPS tracking job for {}: {}", assignment_id, e);
                                                    } else {
                                                        println!("✅ GPS tracking job enqueued for {}", assignment_id);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    println!("⚠️  Cron job: No answers array found in result");
                                }
                            } else {
                                println!("⚠️  Cron job: No 'ok' field in result");
                            }
                        } else {
                            println!("❌ Cron job: Failed to query assignments");
                        }
                    }
                }
            });
        }
        
    }
    
    /// Enqueue a GPS tracking job for a specific assignment
    pub async fn enqueue_gps_tracking(&self, assignment_id: String) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        let job = GPSTrackingJob::new(assignment_id);
        
        self.adapter.enqueue(ctx, job).await?;
        Ok(())
    }
    
    /// Get system statistics
    pub async fn get_stats(&self) -> Result<Value> {
        Ok(serde_json::json!({
            "status": "active",
            "workers": self.worker_handles.len(),
            "backend": "memory",
            "registered_jobs": [
                "gps_tracking",
                "employee_assignment", 
                "route_rebalancing",
                "sla_monitoring",
                "maintenance_scheduling",
                "compliance_monitoring"
            ]
        }))
    }
    
    /// Shutdown background system
    pub async fn shutdown(self) -> Result<()> {
        for handle in self.worker_handles {
            handle.shutdown().await?;
        }
        println!("Background processing system shutdown complete");
        Ok(())
    }
}
