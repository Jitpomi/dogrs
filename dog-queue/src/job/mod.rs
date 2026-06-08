pub mod registry;

pub use registry::{JobHandler, JobRegistry};

use crate::{JobError, JobPriority};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

/// Trait for defining jobs that can be processed by the queue
#[async_trait]
pub trait Job: Send + Sync + Serialize + DeserializeOwned + 'static {
    /// Context type passed to job execution
    type Context: Send + Sync + Clone + 'static;

    /// Result type returned by job execution.
    ///
    /// Must be both `Serialize` (for storage in the backend after `ack_complete`)
    /// and `DeserializeOwned` (for retrieval via `QueueAdapter::get_result`).
    /// Expressing both bounds here surfaces the requirement at the `impl Job`
    /// site rather than deferring it to the `get_result` call site.
    type Result: Send + Serialize + DeserializeOwned + 'static;

    /// Job type identifier for dispatch
    const JOB_TYPE: &'static str;

    /// Job priority
    const PRIORITY: JobPriority = JobPriority::Normal;

    /// Number of **additional** retries after the initial attempt.
    ///
    /// Convention (Bull, Sidekiq, Celery): `MAX_RETRIES = 3` → 4 total executions
    /// (1 initial + 3 retries). This is intentional — set `MAX_RETRIES = 0` for
    /// "run once and never retry".
    ///
    /// The adapter uses `attempt <= MAX_RETRIES` to gate the retry schedule:
    /// attempt 1 → retry if `1 <= MAX_RETRIES`, ..., attempt MAX_RETRIES → retry,
    /// attempt MAX_RETRIES + 1 → permanent failure.
    const MAX_RETRIES: u32 = 3;

    /// Execute the job with the given context
    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError>;

    /// Get idempotency key (optional)
    fn idempotency_key(&self) -> Option<String> {
        None
    }
}
