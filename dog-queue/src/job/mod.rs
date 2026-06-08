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

    /// Result type returned by job execution
    type Result: Send + Serialize + 'static;

    /// Job type identifier for dispatch
    const JOB_TYPE: &'static str;

    /// Job priority
    const PRIORITY: JobPriority = JobPriority::Normal;

    /// Maximum retry attempts
    const MAX_RETRIES: u32 = 3;

    /// Execute the job with the given context
    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError>;

    /// Get idempotency key (optional)
    fn idempotency_key(&self) -> Option<String> {
        None
    }
}
