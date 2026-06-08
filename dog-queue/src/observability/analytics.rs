use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;

use crate::{JobEvent, JobId, QueueCtx};

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

    /// Record job enqueued event.
    ///
    /// `queue` is the real queue name from the encoded `JobMessage` (not a
    /// hardcoded default); callers must pass `message.queue` or the equivalent.
    pub async fn record_job_enqueued(
        &self,
        ctx: &QueueCtx,
        job_id: &JobId,
        job_type: &str,
        queue: &str,
    ) {
        let event = JobEvent::Enqueued {
            job_id: job_id.clone(),
            tenant_id: ctx.tenant_id.clone(),
            queue: queue.to_string(),
            job_type: job_type.to_string(),
            at: Utc::now(),
        };

        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_enqueued(job_type);
        debug!("Recorded job enqueued: {} ({}) queue={}", job_id, job_type, queue);
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

    /// Record job failed event.
    ///
    /// `error` must be the real job error string from `JobError::to_string()`
    /// so that the event stream carries actionable failure information.
    pub async fn record_job_failed(
        &self,
        _ctx: &QueueCtx,
        job_id: &JobId,
        job_type: &str,
        error: &str,
    ) {
        let event = JobEvent::Failed {
            job_id: job_id.clone(),
            error: error.to_string(),
            at: Utc::now(),
        };

        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_failed(job_type);
        debug!("Recorded job failed: {} ({}) error={}", job_id, job_type, error);
    }

    /// Record job retrying event.
    ///
    /// Both `retry_at` and `error` must come from the adapter's actual backoff
    /// calculation and error value — not fabricated inside this method.
    pub async fn record_job_retrying(
        &self,
        _ctx: &QueueCtx,
        job_id: &JobId,
        job_type: &str,
        error: &str,
        retry_at: DateTime<Utc>,
    ) {
        let event = JobEvent::Retrying {
            job_id: job_id.clone(),
            retry_at,
            error: error.to_string(),
            at: Utc::now(),
        };

        let _ = self.event_broadcaster.send(event);
        self.metrics.increment_jobs_retried(job_type);
        debug!(
            "Recorded job retrying: {} ({}) retry_at={} error={}",
            job_id, job_type, retry_at, error
        );
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
