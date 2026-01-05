use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use serde::Deserialize;

use crate::{QueueResult, QueueError, Job, JobError, JobMessage};

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
impl<J: Job + for<'de> Deserialize<'de>> JobHandler for ConcreteJobHandler<J> {
    async fn execute(
        &self,
        message: &JobMessage,
        context: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Option<String>, JobError> {
        // Deserialize the job from payload
        let job: J = serde_json::from_slice(&message.payload_bytes)
            .map_err(|e| JobError::Permanent(format!("Failed to deserialize job: {}", e)))?;
        
        // Downcast the context
        let typed_context = context
            .downcast_ref::<J::Context>()
            .ok_or_else(|| JobError::Permanent("Invalid context type".to_string()))?
            .clone();
        
        // Execute the job
        let result = job.execute(typed_context).await
?;
        
        // For now, we'll serialize the result as JSON
        // In production, this might be stored in dog-blob and return a reference
        let result_json = serde_json::to_string(&result)
            .map_err(|e| JobError::Permanent(format!("Failed to serialize result: {}", e)))?;
        
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
    pub fn register<J: Job + for<'de> Deserialize<'de> + 'static>(&mut self) -> QueueResult<()> {
        let handler = Arc::new(ConcreteJobHandler::<J>::new());
        let job_type = handler.job_type().to_string();
        
        if self.handlers.contains_key(&job_type) {
            return Err(QueueError::Internal(format!("Job type '{}' already registered", job_type)));
        }
        
        self.handlers.insert(job_type, handler);
        Ok(())
    }
    
    /// Execute a job by message
    pub async fn execute_job(
        &self,
        message: &JobMessage,
        context: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Option<String>, JobError> {
        let handler = self.handlers
            .get(&message.job_type)
            .ok_or_else(|| JobError::Permanent(format!("Unknown job type: {}", message.job_type)))?;
        
        handler.execute(message, context).await
    }
    
    /// Check if a job type is registered
    pub fn is_registered(&self, job_type: &str) -> bool {
        self.handlers.contains_key(job_type)
    }
    
    /// Get all registered job types
    pub fn registered_types(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
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
    use crate::{JobPriority, JobError};

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
        
        // Check registration
        assert!(registry.is_registered("test_job"));
        assert_eq!(registry.registered_types(), vec!["test_job"]);
        
        // Create test message
        let job = TestJob { data: "test".to_string() };
        let message = JobMessage {
            job_type: "test_job".to_string(),
            payload_bytes: serde_json::to_vec(&job).unwrap(),
            codec: "json".to_string(),
            queue: "default".to_string(),
            priority: JobPriority::Normal,
            max_retries: 3,
            run_at: chrono::Utc::now(),
            idempotency_key: None,
        };
        
        // Execute job
        let context = Arc::new("test_context".to_string()) as Arc<dyn std::any::Any + Send + Sync>;
        let result = registry.execute_job(&message, context).await.unwrap();
        
        assert!(result.is_some());
        assert!(result.unwrap().contains("Processed: test with context: test_context"));
    }


    #[tokio::test]
    async fn test_unregistered_job_type() {
        let registry = JobRegistry::new();
        
        let payload_bytes = vec![1, 2, 3]; // Invalid payload
        let ctx = "test_context".to_string();
        
        let message = JobMessage {
            job_type: "unknown_job".to_string(),
            payload_bytes,
            codec: "json".to_string(),
            queue: "default".to_string(),
            priority: JobPriority::Normal,
            max_retries: 3,
            run_at: chrono::Utc::now(),
            idempotency_key: None,
        };
        let result = registry.execute_job(&message, Arc::new(ctx)).await;
        assert!(result.is_err());
        
        match result.unwrap_err() {
            JobError::Permanent(msg) => {
                assert!(msg.contains("Unknown job type"));
            }
            _ => panic!("Expected permanent error"),
        }
    }
}
