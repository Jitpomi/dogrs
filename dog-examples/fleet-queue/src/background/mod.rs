/// Fleet background jobs; select a persistent backend for restart recovery.
///
/// This module provides proper dog-queue integration following the actual API patterns.
pub mod jobs;

use crate::services::FleetParams;
use anyhow::Result;
use dog_core::DogApp;
#[cfg(not(feature = "postgres"))]
use dog_queue::backend::memory::MemoryBackend as FleetBackend;
#[cfg(feature = "postgres")]
use dog_queue::backend::postgres::PostgresBackend as FleetBackend;
use dog_queue::{EnqueueOptions, Job, QueueAdapter, QueueCtx, WorkerHandle};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub use jobs::*;

/// Unified context for all background jobs
#[derive(Clone)]
pub struct FleetContext {
    pub app: DogApp<Value, FleetParams>,
    pub tenant_id: String,
}

/// Main background processing system using proper dog-queue patterns
pub struct BackgroundSystem {
    adapter: Arc<QueueAdapter<FleetBackend>>,
    worker_handles: Mutex<Vec<WorkerHandle>>,
}

impl BackgroundSystem {
    /// Create new background system with proper dog-queue integration
    pub async fn new() -> Result<Self> {
        // Create memory backend for now (can be swapped for Redis/PostgreSQL)
        #[cfg(not(feature = "postgres"))]
        let backend = {
            eprintln!("fleet-queue: memory mode; queued jobs disappear on restart. Use --features postgres for local durable storage.");
            FleetBackend::new()
        };
        #[cfg(feature = "postgres")]
        let backend = {
            let connection_string = std::env::var("FLEET_POSTGRES_URL")?;
            let parsed: tokio_postgres::Config = connection_string.parse()?;
            anyhow::ensure!(!parsed.get_hosts().is_empty() && parsed.get_hosts().iter().all(|host| match host {
                tokio_postgres::config::Host::Tcp(host) => host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()),
                #[cfg(unix)]
                tokio_postgres::config::Host::Unix(_) => true,
            }), "This example's PostgreSQL connector is local only; use a verified TLS connector for remote PostgreSQL (see hosted-system)");
            FleetBackend::new(dog_queue::backend::postgres::PostgresConfig { connection_string })
                .await?
        };

        // Use custom configuration with long idle timeout so workers don't shutdown during testing
        let config = dog_queue::QueueConfig {
            worker_idle_timeout: std::time::Duration::from_secs(86400),
            ..Default::default()
        };
        let adapter = Arc::new(QueueAdapter::try_with_config(backend, config)?);

        // Register all implemented job types
        adapter.register_job::<GPSTrackingJob>().await?;
        adapter.register_job::<EmployeeAssignmentJob>().await?;
        adapter.register_job::<RouteRebalancingJob>().await?;
        adapter.register_job::<SLAMonitoringJob>().await?;
        adapter.register_job::<MaintenanceSchedulingJob>().await?;
        adapter.register_job::<ComplianceMonitoringJob>().await?;

        Ok(Self {
            adapter,
            worker_handles: Mutex::new(Vec::new()),
        })
    }

    /// Start background processing workers
    pub async fn start(&self, app: DogApp<Value, FleetParams>) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        let context = FleetContext {
            app,
            tenant_id: "fleet_tenant".to_string(),
        };

        // Start workers for all implemented job types - use JOB_TYPE constants
        let queues = vec![
            GPSTrackingJob::JOB_TYPE.to_string(),
            EmployeeAssignmentJob::JOB_TYPE.to_string(),
            RouteRebalancingJob::JOB_TYPE.to_string(),
            SLAMonitoringJob::JOB_TYPE.to_string(),
            MaintenanceSchedulingJob::JOB_TYPE.to_string(),
            ComplianceMonitoringJob::JOB_TYPE.to_string(),
        ];

        let worker_handle = self
            .adapter
            .start_workers(ctx.clone(), context, queues)
            .await?;

        self.worker_handles.lock().unwrap().push(worker_handle);
        Ok(())
    }

    /// Enqueue a GPS tracking job for a specific assignment
    pub async fn enqueue_gps_tracking(&self, assignment_id: String) -> Result<()> {
        self.enqueue_gps_tracking_opts(assignment_id, EnqueueOptions::immediate())
            .await
    }

    /// Enqueue a GPS tracking job with options
    pub async fn enqueue_gps_tracking_opts(
        &self,
        assignment_id: String,
        opts: EnqueueOptions,
    ) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        let job = GPSTrackingJob::new(assignment_id);

        self.adapter.enqueue_opts(ctx, job, opts).await?;
        Ok(())
    }

    /// Enqueue a Route Rebalancing job
    pub async fn enqueue_route_rebalancing(
        &self,
        affected_routes: Vec<String>,
        traffic_delay_minutes: i32,
        trigger_reason: String,
    ) -> Result<()> {
        self.enqueue_route_rebalancing_opts(
            affected_routes,
            traffic_delay_minutes,
            trigger_reason,
            EnqueueOptions::immediate(),
        )
        .await
    }

    /// Enqueue a Route Rebalancing job with options
    pub async fn enqueue_route_rebalancing_opts(
        &self,
        affected_routes: Vec<String>,
        traffic_delay_minutes: i32,
        trigger_reason: String,
        opts: EnqueueOptions,
    ) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        let job = RouteRebalancingJob::new(affected_routes, traffic_delay_minutes, trigger_reason);

        self.adapter.enqueue_opts(ctx, job, opts).await?;
        Ok(())
    }

    /// Enqueue a Route Rebalancing job with options and repeats
    pub async fn enqueue_route_rebalancing_repeat(
        &self,
        affected_routes: Vec<String>,
        traffic_delay_minutes: i32,
        trigger_reason: String,
        repeat_interval_seconds: Option<u64>,
        repeat_count: Option<u32>,
        max_repeats: Option<u32>,
        opts: EnqueueOptions,
    ) -> Result<()> {
        let ctx = QueueCtx::new("fleet_tenant".to_string());
        let job = RouteRebalancingJob {
            affected_routes,
            traffic_delay_minutes,
            trigger_reason,
            repeat_interval_seconds,
            repeat_count,
            max_repeats,
        };

        self.adapter.enqueue_opts(ctx, job, opts).await?;
        Ok(())
    }

    /// Get system statistics
    pub async fn get_stats(&self) -> Result<Value> {
        Ok(serde_json::json!({
            "status": "active",
            "workers": self.worker_handles.lock().unwrap().len(),
            "backend": "memory",
            "registered_jobs": [
                GPSTrackingJob::JOB_TYPE,
                EmployeeAssignmentJob::JOB_TYPE,
                RouteRebalancingJob::JOB_TYPE,
                SLAMonitoringJob::JOB_TYPE,
                MaintenanceSchedulingJob::JOB_TYPE,
                ComplianceMonitoringJob::JOB_TYPE,
            ]
        }))
    }

    /// Shutdown background system
    pub async fn shutdown(&self) -> Result<()> {
        let handles: Vec<_> = self.worker_handles.lock().unwrap().drain(..).collect();
        for handle in handles {
            handle.shutdown().await?;
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::*;
    use dog_queue::QueueBackend;
    #[tokio::test]
    #[ignore = "requires disposable local PostgreSQL via FLEET_POSTGRES_URL"]
    async fn queued_job_survives_backend_recreation() -> Result<()> {
        let tenant = format!(
            "fleet-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let ctx = QueueCtx::new(tenant);
        let first = BackgroundSystem::new().await?;
        let id = first
            .adapter
            .enqueue(
                ctx.clone(),
                GPSTrackingJob::new("example-assignment".into()),
            )
            .await?;
        drop(first);
        let second = BackgroundSystem::new().await?;
        let snapshot = second
            .adapter
            .backend()
            .get_snapshot(ctx.clone(), id.clone())
            .await?;
        assert_eq!(snapshot.job_id, id);
        second.adapter.cancel(ctx, id).await?;
        second.shutdown().await?;
        Ok(())
    }
}
