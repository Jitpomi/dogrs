pub mod memory;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_core::Stream;
use std::pin::Pin;
use std::time::Duration;

use crate::{
    QueueResult, QueueCtx, JobId, JobMessage, JobStatus,
    LeasedJob, QueueCapabilities, JobEvent, JobRecord,
    types::LeaseToken
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

    /// Extend lease duration (optional capability)
    async fn heartbeat_extend(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
        lease_token: LeaseToken,
        extra_time: Duration,
    ) -> QueueResult<()>;

    /// Cancel a job (cancel-wins semantics)
    async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool>;

    /// Get job status
    async fn get_status(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus>;

    /// Get full job record (optional - for observability/UI/debugging)
    async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord>;

    /// Event stream for observability (boxed for stable Rust)
    fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent>;

    /// Get backend capabilities
    fn capabilities(&self) -> QueueCapabilities;
}
