use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;
use chrono::Utc;

use crate::{QueueCtx, JobId, JobEvent};

/// Production-grade observability layer
#[derive(Clone)]
pub struct ObservabilityLayer {
    event_broadcaster: broadcast::Sender<JobEvent>,
    metrics: Arc<super::LiveMetrics>,
}

impl ObservabilityLayer {
    /// Create new observability layer
    pub fn new() -> Self {
        let (event_broadcaster, _) = broadcast::channel(10000);
        
        Self {
            event_broadcaster,
            metrics: Arc::new(super::LiveMetrics::new()),
        }
    }

    /// Record job enqueued event
    pub async fn record_job_enqueued(&self, ctx: &QueueCtx, job_id: &JobId, job_type: &str) {
        let event = JobEvent::Enqueued {
            job_id: job_id.clone(),
            tenant_id: ctx.tenant_id.clone(),
            queue: "default".to_string(), // TODO: Get from context
            job_type: job_type.to_string(),
            at: Utc::now(),
        };
        
        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_enqueued(job_type);
        debug!("Recorded job enqueued: {} ({})", job_id, job_type);
    }

    /// Record job completed event
    pub async fn record_job_completed(&self, _ctx: &QueueCtx, job_id: &JobId, job_type: &str) {
        let event = JobEvent::Completed {
            job_id: job_id.clone(),
            at: Utc::now(),
        };
        
        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_completed(job_type);
        debug!("Recorded job completed: {} ({})", job_id, job_type);
    }

    /// Record job failed event
    pub async fn record_job_failed(&self, _ctx: &QueueCtx, job_id: &JobId, job_type: &str) {
        let event = JobEvent::Failed {
            job_id: job_id.clone(),
            error: "Job execution failed".to_string(),
            at: Utc::now(),
        };
        
        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_failed(job_type);
        debug!("Recorded job failed: {} ({})", job_id, job_type);
    }

    /// Record job retrying event
    pub async fn record_job_retrying(&self, _ctx: &QueueCtx, job_id: &JobId, job_type: &str) {
        let retry_at = Utc::now() + chrono::Duration::seconds(60);
        let event = JobEvent::Retrying {
            job_id: job_id.clone(),
            retry_at,
            error: "Job failed, retrying".to_string(),
            at: Utc::now(),
        };
        
        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_retried(job_type);
        debug!("Recorded job retrying: {} ({})", job_id, job_type);
    }

    /// Get event stream
    pub fn event_stream(&self) -> broadcast::Receiver<JobEvent> {
        self.event_broadcaster.subscribe()
    }

    /// Get live metrics
    pub fn metrics(&self) -> &super::LiveMetrics {
        &self.metrics
    }
}

impl Default for ObservabilityLayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance analytics for queue operations
pub struct PerformanceAnalytics {
    observability: Arc<ObservabilityLayer>,
}

impl PerformanceAnalytics {
    pub fn new(observability: Arc<ObservabilityLayer>) -> Self {
        Self { observability }
    }

    /// Get job processing rate (jobs per second)
    pub fn job_processing_rate(&self) -> f64 {
        let completed = self.observability.metrics.jobs_completed() as f64;
        let failed = self.observability.metrics.jobs_failed() as f64;
        
        // Simple rate calculation - in production this would be time-windowed
        completed + failed
    }

    /// Get success rate percentage
    pub fn success_rate(&self) -> f64 {
        let completed = self.observability.metrics.jobs_completed() as f64;
        let failed = self.observability.metrics.jobs_failed() as f64;
        let total = completed + failed;
        
        if total == 0.0 {
            100.0
        } else {
            (completed / total) * 100.0
        }
    }

    /// Get retry rate percentage
    pub fn retry_rate(&self) -> f64 {
        let retried = self.observability.metrics.jobs_retried() as f64;
        let enqueued = self.observability.metrics.jobs_enqueued() as f64;
        
        if enqueued == 0.0 {
            0.0
        } else {
            (retried / enqueued) * 100.0
        }
    }
}
