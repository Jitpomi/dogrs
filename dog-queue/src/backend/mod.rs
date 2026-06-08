pub mod memory;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_core::Stream;
use std::pin::Pin;
use std::time::Duration;

use crate::{
    types::LeaseToken, JobEvent, JobId, JobMessage, JobRecord, JobStatus, LeasedJob,
    QueueCapabilities, QueueCtx, QueueError, QueueResult,
};

/// Type alias for boxed streams (stable Rust compatible)
pub type BoxStream<T> = Pin<Box<dyn Stream<Item = T> + Send + 'static>>;

/// Backend trait for queue storage primitives
#[async_trait]
pub trait QueueBackend: Send + Sync {
    /// Enqueue a job with tenant-scoped idempotency
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId>;

    /// Lease-based dequeue (eligible jobs only)
    /// Returns jobs with run_at <= now and not in terminal status
    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>>;

    /// Acknowledge job completion (cancel-wins, lease token required)
    async fn ack_complete(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
        lease_token: LeaseToken,
        result_ref: Option<String>,
    ) -> QueueResult<()>;

    /// Acknowledge job failure with optional retry scheduling
    /// retry_at is computed by adapter (backoff policy lives in adapter)
    async fn ack_fail(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
        lease_token: LeaseToken,
        error: String,
        retry_at: Option<DateTime<Utc>>,
    ) -> QueueResult<()>;

    /// Extend lease duration.
    ///
    /// Only required for backends that advertise `QueueCapabilities::lease_extend = true`.
    /// The default implementation returns [`QueueError::BackendUnsupported`] so backends
    /// that do not implement heartbeating get a graceful, diagnosable error rather than a
    /// compile error or `unimplemented!()` panic.
    async fn heartbeat_extend(
        &self,
        _ctx: QueueCtx,
        _job_id: JobId,
        _lease_token: LeaseToken,
        _extra_time: Duration,
    ) -> QueueResult<()> {
        Err(QueueError::BackendUnsupported(
            "heartbeat_extend: this backend does not support lease extension".to_string(),
        ))
    }

    /// Cancel a job (cancel-wins semantics)
    async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool>;

    /// Get job status
    async fn get_status(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus>;

    /// Get full job record.
    ///
    /// **Optional** — backends that do not support full record retrieval should
    /// leave this default in place. The default returns [`QueueError::BackendUnsupported`]
    /// so callers get a clear, diagnosable error rather than a compile error or panic.
    /// Backends intended for observability/UI should override this.
    async fn get_record(&self, _ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
        Err(QueueError::BackendUnsupported(format!(
            "get_record: this backend does not expose full job records (job_id: {job_id})",
        )))
    }

    /// Event stream for observability (boxed for stable Rust)
    fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent>;

    /// Reclaim expired leases by detecting timed-out jobs and re-queuing them for retry.
    ///
    /// Backends that manage lease expiry internally (e.g. [`MemoryBackend`]) should
    /// override this.  The default is a no-op (`Ok(0)`) for backends that rely on an
    /// external TTL mechanism (Redis `EXPIRE`, Postgres `pg_cron`) which handles
    /// reclamation outside the Rust process.
    ///
    /// Called periodically by `QueueAdapter::start_workers` at `lease_duration / 2`
    /// intervals.  Returns the number of leases reclaimed in this cycle.
    async fn reclaim_expired_leases(&self) -> QueueResult<usize> {
        Ok(0)
    }

    /// Get backend capabilities
    fn capabilities(&self) -> QueueCapabilities;
}
