use std::sync::Arc;

use crate::{
    backend::QueueBackend, codec::CodecRegistry, observability::ObservabilityLayer, Job, JobId,
    QueueCtx, QueueError, QueueResult,
};

/// Production-grade queue engine with multi-tenant semantics
pub struct QueueEngine<B: QueueBackend> {
    backend: B,
    codec_registry: Arc<CodecRegistry>,
    observability: Arc<ObservabilityLayer>,
}

impl<B: QueueBackend> QueueEngine<B> {
    /// Create a new queue engine with the given backend
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            codec_registry: Arc::new(CodecRegistry::new()),
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
        self.observability
            .record_job_enqueued(&ctx, &job_id, J::JOB_TYPE)
            .await;

        Ok(job_id)
    }

    /// Execute a job immediately, bypassing durable storage.
    ///
    /// **For development and testing only.** This path skips `enqueue`, `dequeue`,
    /// and `ack_complete`, so the job has no `JobId` and is invisible to observability
    /// and the normal worker pipeline. In production always use `enqueue`.
    pub async fn execute_now<J: Job>(
        &self,
        ctx: QueueCtx,
        job: J,
        execution_context: J::Context,
    ) -> QueueResult<J::Result> {
        let _ = ctx; // ctx unused here; kept for API symmetry with enqueue

        // Execute directly. No observability recording: this method has no real JobId
        // (the job was never durably enqueued) and a phantom ID would corrupt dashboards.
        job.execute(execution_context)
            .await
            .map_err(QueueError::JobFailed)
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
