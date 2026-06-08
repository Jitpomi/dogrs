use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::debug;

use crate::{JobId, QueueCtx};

/// Metrics-only observability layer.
///
/// Tracks job lifecycle counters via `LiveMetrics`. Event streaming uses the
/// backend's own `event_stream()` — having a second independent broadcast channel
/// here caused dual emission of every event and inconsistent buffer sizes.
#[derive(Clone)]
pub struct ObservabilityLayer {
    metrics: Arc<super::LiveMetrics>,
}

impl ObservabilityLayer {
    /// Create new observability layer
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(super::LiveMetrics::new()),
        }
    }

    /// Record job enqueued event.
    ///
    /// `queue` is the real queue name from the encoded `JobMessage` (not a
    /// hardcoded default); callers must pass `message.queue` or the equivalent.
    pub fn record_job_enqueued(
        &self,
        ctx: &QueueCtx,
        job_id: &JobId,
        job_type: &str,
        queue: &str,
    ) {
        self.metrics.increment_jobs_enqueued(job_type);
        debug!("Recorded job enqueued: {} ({}) queue={}", job_id, job_type, queue);
        let _ = (ctx, job_id); // fields used for logging / future extensions
    }

    /// Record job completed event
    pub fn record_job_completed(&self, _ctx: &QueueCtx, job_id: &JobId, job_type: &str) {
        self.metrics.increment_jobs_completed(job_type);
        debug!("Recorded job completed: {} ({})", job_id, job_type);
    }

    /// Record job failed event.
    ///
    /// `error` must be the real job error string from `JobError::to_string()`
    /// so that the event stream carries actionable failure information.
    pub fn record_job_failed(
        &self,
        _ctx: &QueueCtx,
        job_id: &JobId,
        job_type: &str,
        error: &str,
    ) {
        self.metrics.increment_jobs_failed(job_type);
        debug!("Recorded job failed: {} ({}) error={}", job_id, job_type, error);
    }

    /// Record job retrying event.
    ///
    /// Both `retry_at` and `error` must come from the adapter's actual backoff
    /// calculation and error value — not fabricated inside this method.
    pub fn record_job_retrying(
        &self,
        _ctx: &QueueCtx,
        job_id: &JobId,
        job_type: &str,
        error: &str,
        retry_at: DateTime<Utc>,
    ) {
        self.metrics.increment_jobs_retried(job_type);
        debug!(
            "Recorded job retrying: {} ({}) retry_at={} error={}",
            job_id, job_type, retry_at, error
        );
    }

    /// Record job canceled event.
    ///
    /// Called by [`QueueAdapter::cancel`] when the backend reports a successful
    /// cancellation.  This is the only path that increments `jobs_canceled`;
    /// previously the counter was permanently zero because `cancel` was not
    /// exposed on the adapter.
    pub fn record_job_canceled(&self, _ctx: &QueueCtx, job_id: &JobId, job_type: &str) {
        self.metrics.increment_jobs_canceled(job_type);
        debug!("Recorded job canceled: {} ({})", job_id, job_type);
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

    /// Total jobs processed (completed + failed) since process start.
    ///
    /// This is a monotonically-increasing lifetime count, **not a rate**.
    /// To compute a true throughput (jobs/second), snapshot this value twice
    /// with a known elapsed duration and divide the delta by the elapsed seconds.
    ///
    /// Uses a single coherent [`GlobalMetrics`](crate::observability::metrics::GlobalMetrics)
    /// snapshot so `completed` and `failed` are read from the same atomic state.
    pub fn total_jobs_processed(&self) -> u64 {
        let (global, _) = self.observability.metrics.snapshot_all();
        global.jobs_completed + global.jobs_failed
    }

    /// Get success rate percentage.
    ///
    /// Returns `0.0` when no jobs have completed or failed (no data ≠ perfect record).
    ///
    /// Delegates to [`GlobalMetrics::success_rate`](crate::observability::metrics::GlobalMetrics::success_rate)
    /// on a coherent [`snapshot_all`](crate::LiveMetrics::snapshot_all) snapshot
    /// so `completed` and `failed` are read from the same atomic state.
    pub fn success_rate(&self) -> f64 {
        let (global, _) = self.observability.metrics.snapshot_all();
        global.success_rate()
    }

    /// Retry event rate: retry events per terminal job (completed + failed).
    ///
    /// Because `jobs_retried` is incremented once per retry *event* (a job
    /// retried three times contributes 3), dividing by `jobs_enqueued` would
    /// yield values well above 100% for retryable workloads. The correct
    /// denominator is the number of terminal jobs (completed or permanently
    /// failed), which equals the total number of original job attempts.
    ///
    /// Delegates to [`GlobalMetrics::retry_rate`](crate::observability::metrics::GlobalMetrics::retry_rate)
    /// on a coherent snapshot so all three counters are read atomically.
    pub fn retry_rate(&self) -> f64 {
        let (global, _) = self.observability.metrics.snapshot_all();
        global.retry_rate()
    }
}
