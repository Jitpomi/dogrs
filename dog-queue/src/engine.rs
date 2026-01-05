use std::sync::Arc;
use parking_lot::RwLock;

use crate::{
    QueueResult, QueueError, QueueCtx, JobId, Job,
    backend::QueueBackend,
    codec::CodecRegistry,
    job::JobRegistry,
    observability::ObservabilityLayer,
};

/// Production-grade queue engine with multi-tenant semantics
pub struct QueueEngine<B: QueueBackend> {
    backend: B,
    codec_registry: Arc<CodecRegistry>,
    #[allow(dead_code)]
    job_registry: Arc<RwLock<JobRegistry>>,
    observability: Arc<ObservabilityLayer>,
}

impl<B: QueueBackend> QueueEngine<B> {
    /// Create a new queue engine with the given backend
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            codec_registry: Arc::new(CodecRegistry::new()),
            job_registry: Arc::new(RwLock::new(JobRegistry::new())),
            observability: Arc::new(ObservabilityLayer::new()),
        }
    }

    /// Create engine with custom codec registry
    pub fn with_codec_registry(mut self, registry: CodecRegistry) -> Self {
        self.codec_registry = Arc::new(registry);
        self
    }

    /// Create engine with observability layer
    pub fn with_observability(mut self, observability: ObservabilityLayer) -> Self {
        self.observability = Arc::new(observability);
        self
    }

    /// Enqueue a job for processing (proper queue semantics)
    pub async fn enqueue<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<JobId> {
        // Encode job using codec registry
        let message = self.codec_registry.encode_job(&job, &ctx)?;
        
        // Enqueue to backend
        let job_id = self.backend.enqueue(ctx.clone(), message).await?;
        
        // Emit observability event
        self.observability.record_job_enqueued(&ctx, &job_id, J::JOB_TYPE).await;
        
        Ok(job_id)
    }

    /// Execute job immediately (for tests/dev - bypasses durable storage)
    pub async fn execute_now<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<J::Result> {
        // This is for local testing/development - direct execution
        // In production, jobs are processed by workers via dequeue
        
        // Create a mock context for execution
        let execution_context = self.create_execution_context::<J>(ctx.clone());
        
        // Execute directly with proper error handling
        let result = job.execute(execution_context).await
            .map_err(QueueError::JobFailed)?;
        
        // Emit observability event
        self.observability.record_job_completed(&ctx, &JobId::new(), J::JOB_TYPE).await;
        
        Ok(result)
    }

    /// Create execution context for a job type
    fn create_execution_context<J: Job>(&self, _ctx: QueueCtx) -> J::Context {
        todo!("Context creation needs to be implemented based on job requirements")
    }

    /// Get backend reference
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Get codec registry
    pub fn codec_registry(&self) -> &CodecRegistry {
        &self.codec_registry
    }

    /// Get observability layer
    pub fn observability(&self) -> &ObservabilityLayer {
        &self.observability
    }
}


