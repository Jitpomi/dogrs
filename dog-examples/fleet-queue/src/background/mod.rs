/// Production-ready background processing system for fleet operations
/// 
/// This module provides proper dog-queue integration following the actual API patterns.

pub mod jobs;

use anyhow::Result;
use dog_queue::prelude::*;
use dog_queue::backend::memory::MemoryBackend;
use dog_queue::WorkerHandle;
use serde_json::Value;
use std::sync::Arc;
use dog_axum::AxumApp;
use crate::services::FleetParams;

pub use jobs::*;

/// Unified context for all background jobs
#[derive(Clone)]
pub struct FleetContext {
    pub app: Arc<AxumApp<Value, FleetParams>>,
    pub tenant_id: String,
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
        
        // Use dog-queue's default configuration - no need for excessive env var parsing
        let adapter = Arc::new(QueueAdapter::new(backend));
        
        // Register all implemented job types
        adapter.register_job::<GPSTrackingJob>().await?;
        adapter.register_job::<EmployeeAssignmentJob>().await?;
        adapter.register_job::<RouteRebalancingJob>().await?;
        adapter.register_job::<SLAMonitoringJob>().await?;
        adapter.register_job::<MaintenanceSchedulingJob>().await?;
        adapter.register_job::<ComplianceMonitoringJob>().await?;
        
        let context = FleetContext {
            app,
            tenant_id: "fleet_tenant".to_string(),
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
        
        // Start workers for all implemented job types - use JOB_TYPE constants
        let queues = vec![
            GPSTrackingJob::JOB_TYPE.to_string(),
            EmployeeAssignmentJob::JOB_TYPE.to_string(),
            RouteRebalancingJob::JOB_TYPE.to_string(),
            SLAMonitoringJob::JOB_TYPE.to_string(),
            MaintenanceSchedulingJob::JOB_TYPE.to_string(),
            ComplianceMonitoringJob::JOB_TYPE.to_string(),
        ];
        
        println!("🔧 Starting dog-queue workers for queues: {:?}", queues);
        let worker_handle = self.adapter.start_workers(
            ctx.clone(),
            self.context.clone(),
            queues,
        ).await?;
        
        self.worker_handles.push(worker_handle);
        
        
        println!("🚀 Background processing system started with proper dog-queue integration");
        println!("📊 Workers active: {}", self.worker_handles.len());
        Ok(())
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
