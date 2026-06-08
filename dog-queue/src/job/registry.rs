use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{Job, JobError, JobMessage, QueueError, QueueResult};

/// Type-erased job handler for runtime dispatch
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Execute a job with the given message and context
    async fn execute(
        &self,
        message: &JobMessage,
        context: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Option<String>, JobError>;

    /// Get the job type this handler processes
    fn job_type(&self) -> &'static str;
}

/// Concrete job handler implementation
struct ConcreteJobHandler<J: Job> {
    _phantom: std::marker::PhantomData<J>,
}

impl<J: Job> ConcreteJobHandler<J> {
    fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<J: Job> JobHandler for ConcreteJobHandler<J> {
    async fn execute(
        &self,
        message: &JobMessage,
        context: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Option<String>, JobError> {
        // Deserialize the job from payload
        let job: J = serde_json::from_slice(&message.payload_bytes)
            .map_err(|e| JobError::Permanent(format!("Failed to deserialize job: {}", e)))?;

        // Downcast the context to the concrete type this job expects.
        // If this fails, the worker was started with the wrong context type —
        // include type info so the error is diagnosable rather than looking
        // like a real job failure that burns the retry budget.
        let typed_context = context
            .downcast_ref::<J::Context>()
            .ok_or_else(|| {
                JobError::Permanent(format!(
                    "Context type mismatch for job '{}': expected context type '{}'",
                    J::JOB_TYPE,
                    std::any::type_name::<J::Context>(),
                ))
            })?
            .clone();

        // Execute the job
        let result = job.execute(typed_context).await?;

        // Serialize the result.  A serialization failure here is a programming
        // error in `J::Result`'s `Serialize` impl — `serde_json::to_string` writes
        // to an in-memory buffer and can only produce Syntax/Data/Eof errors, all
        // of which are deterministic.  Use `Permanent` so the job does not consume
        // its entire retry budget re-executing side effects for an unfixable error.
        let result_json = serde_json::to_string(&result).map_err(|e| {
            JobError::Permanent(format!(
                "Failed to serialize job result (Serialize impl bug — retrying cannot fix this): {e}"
            ))
        })?;

        Ok(Some(result_json))
    }

    fn job_type(&self) -> &'static str {
        J::JOB_TYPE
    }
}

/// Registry for managing job types and their handlers
pub struct JobRegistry {
    handlers: HashMap<String, Arc<dyn JobHandler>>,
}

impl JobRegistry {
    /// Create a new job registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a job type
    pub fn register<J: Job>(&mut self) -> QueueResult<()> {
        let handler = Arc::new(ConcreteJobHandler::<J>::new());
        let job_type = handler.job_type().to_string();

        if self.handlers.contains_key(&job_type) {
            return Err(QueueError::JobTypeAlreadyRegistered(job_type));
        }

        self.handlers.insert(job_type, handler);
        Ok(())
    }

    /// Get a cloned handler for the given job type.
    ///
    /// Clone the handler under the registry lock, drop the lock, then call
    /// `handler.execute(decoded_message, context)` outside the lock.
    /// This prevents long-running jobs from blocking `register_job()` (write lock).
    ///
    /// The `decoded_message` passed to `handler.execute()` must have its
    /// `payload_bytes` pre-decoded through `CodecRegistry::decode_job_payload`
    /// (the adapter's `process_next_job` does this automatically).
    pub fn get_handler(&self, job_type: &str) -> Option<Arc<dyn JobHandler>> {
        self.handlers.get(job_type).cloned()
    }

    /// Check if a job type is registered
    pub fn is_registered(&self, job_type: &str) -> bool {
        self.handlers.contains_key(job_type)
    }

    /// Get all registered job types, sorted alphabetically for deterministic ordering.
    ///
    /// Sorted so that callers (UI, tests, status endpoints) always see a consistent
    /// order regardless of `HashMap` internal state.
    pub fn registered_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.handlers.keys().cloned().collect();
        types.sort_unstable();
        types
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobError, JobPriority};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct TestJob {
        data: String,
    }

    #[async_trait::async_trait]
    impl Job for TestJob {
        type Context = String;
        type Result = String;

        const JOB_TYPE: &'static str = "test_job";
        const PRIORITY: JobPriority = JobPriority::Normal;
        const MAX_RETRIES: u32 = 3;

        async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
            Ok(format!("Processed: {} with context: {}", self.data, ctx))
        }
    }

    #[tokio::test]
    async fn test_job_registry() {
        let mut registry = JobRegistry::new();

        // Register job type
        registry.register::<TestJob>().unwrap();

        // Duplicate registration must be rejected
        assert!(registry.register::<TestJob>().is_err());

        // Check registration
        assert!(registry.is_registered("test_job"));
        assert_eq!(registry.registered_types(), vec!["test_job"]);

        // Create test message (payload already JSON-decoded, as the adapter does)
        let job = TestJob { data: "test".to_string() };
        let message = JobMessage {
            job_type: "test_job".to_string(),
            payload_bytes: serde_json::to_vec(&job).unwrap(), // JSON bytes = decoded form
            codec: "json".to_string(),
            queue: "default".to_string(),
            priority: JobPriority::Normal,
            max_retries: 3,
            run_at: chrono::Utc::now(),
            idempotency_key: None,
        };

        // Correct pattern: clone handler under the lock, drop lock, execute outside.
        let handler = registry.get_handler("test_job").expect("handler must be registered");
        let context = Arc::new("test_context".to_string()) as Arc<dyn std::any::Any + Send + Sync>;
        let result = handler.execute(&message, context).await.unwrap();

        assert!(result.is_some());
        assert!(result.unwrap().contains("Processed: test with context: test_context"));
    }

    #[tokio::test]
    async fn test_unregistered_job_type() {
        let registry = JobRegistry::new();

        // get_handler returns None for unknown types — no error, caller handles it.
        assert!(registry.get_handler("unknown_job").is_none());
    }

    #[tokio::test]
    async fn test_registered_types_sorted() {
        let mut registry = JobRegistry::new();
        registry.register::<TestJob>().unwrap();
        // Even with one entry the sort is exercised and the result is deterministic.
        let types = registry.registered_types();
        assert_eq!(types, vec!["test_job"]);
        // Verify the Vec is sorted (invariant for any number of entries).
        assert!(types.windows(2).all(|w| w[0] <= w[1]));
    }
}
