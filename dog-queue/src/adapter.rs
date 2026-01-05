use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{info, warn, error, debug, instrument};

use crate::{
    QueueResult, QueueError, QueueCtx, JobId, Job,
    backend::QueueBackend,
    codec::CodecRegistry,
    job::JobRegistry,
    observability::ObservabilityLayer,
};

/// Configuration for queue adapter
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Maximum number of concurrent workers per queue
    pub max_workers: usize,
    /// Worker idle timeout before shutdown
    pub worker_idle_timeout: Duration,
    /// Lease duration for jobs
    pub lease_duration: Duration,
    /// Heartbeat interval for lease extension
    pub heartbeat_interval: Duration,
    /// Maximum retry backoff duration
    pub max_retry_backoff: Duration,
    /// Base retry backoff duration
    pub base_retry_backoff: Duration,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_workers: 10,
            worker_idle_timeout: Duration::from_secs(60),
            lease_duration: Duration::from_secs(300), // 5 minutes
            heartbeat_interval: Duration::from_secs(30),
            max_retry_backoff: Duration::from_secs(3600), // 1 hour
            base_retry_backoff: Duration::from_secs(1),
        }
    }
}

/// Handle for managing worker lifecycle
pub struct WorkerHandle {
    shutdown_tx: oneshot::Sender<()>,
    join_handle: JoinHandle<QueueResult<()>>,
}

impl WorkerHandle {
    /// Gracefully shutdown the worker
    pub async fn shutdown(self) -> QueueResult<()> {
        let _ = self.shutdown_tx.send(());
        self.join_handle.await.map_err(|e| QueueError::Internal(format!("Worker join error: {}", e)))?
    }
}

/// Production-grade queue adapter with multi-tenant semantics
pub struct QueueAdapter<B: QueueBackend + ?Sized> {
    backend: Arc<B>,
    codec_registry: Arc<CodecRegistry>,
    job_registry: Arc<RwLock<JobRegistry>>,
    observability: Arc<ObservabilityLayer>,
    config: QueueConfig,
}

impl<B: QueueBackend + Send + Sync + 'static> QueueAdapter<B> {
    /// Create a new queue adapter
    pub fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            codec_registry: Arc::new(CodecRegistry::new()),
            job_registry: Arc::new(RwLock::new(JobRegistry::new())),
            observability: Arc::new(ObservabilityLayer::new()),
            config: QueueConfig::default(),
        }
    }

    /// Create adapter with custom configuration
    pub fn with_config(backend: B, config: QueueConfig) -> Self {
        Self {
            backend: Arc::new(backend),
            codec_registry: Arc::new(CodecRegistry::new()),
            job_registry: Arc::new(RwLock::new(JobRegistry::new())),
            observability: Arc::new(ObservabilityLayer::new()),
            config,
        }
    }

    /// Create adapter with custom codec registry
    pub fn with_codec_registry(mut self, registry: CodecRegistry) -> Self {
        self.codec_registry = Arc::new(registry);
        self
    }

    /// Create adapter with observability layer
    pub fn with_observability(mut self, observability: ObservabilityLayer) -> Self {
        self.observability = Arc::new(observability);
        self
    }

    /// Register a job type for processing
    pub async fn register_job<J: Job>(&self) -> QueueResult<()> {
        let mut registry = self.job_registry.write().await;
        registry.register::<J>()?;
        info!("Registered job type: {}", J::JOB_TYPE);
        Ok(())
    }

    /// Enqueue a job for processing
    #[instrument(skip(self, job), fields(job_type = J::JOB_TYPE, tenant_id = %ctx.tenant_id))]
    pub async fn enqueue<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<JobId> {
        // Encode job using codec registry
        let message = self.codec_registry.encode_job(&job, &ctx)?;
        
        // Enqueue to backend
        let job_id = self.backend.enqueue(ctx.clone(), message).await?;
        
        // Record metrics
        self.observability.record_job_enqueued(&ctx, &job_id, J::JOB_TYPE).await;
        
        info!("Enqueued job {} of type {}", job_id, J::JOB_TYPE);
        Ok(job_id)
    }

    /// Execute job immediately (for tests/dev - bypasses durable storage)
    #[instrument(skip(self, job), fields(job_type = J::JOB_TYPE, tenant_id = %ctx.tenant_id))]
    pub async fn execute_now<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<J::Result> {
        info!("Executing job immediately: {}", J::JOB_TYPE);
        
        // Create execution context
        let execution_context = self.create_execution_context::<J>(ctx.clone()).await?;
        
        // Execute with timeout
        let timeout_duration = Duration::from_secs(300); // Default 5 minute timeout
        let result = tokio::time::timeout(timeout_duration, job.execute(execution_context))
            .await
            .map_err(|_| QueueError::Internal("Job execution timeout".to_string()))?
            .map_err(QueueError::JobFailed)?;
        
        // Record metrics
        self.observability.record_job_completed(&ctx, &JobId::new(), J::JOB_TYPE).await;
        
        info!("Job executed successfully: {}", J::JOB_TYPE);
        Ok(result)
    }

    /// Start workers for processing jobs from specified queues
    #[instrument(skip(self, context), fields(tenant_id = %ctx.tenant_id, queues = ?queues))]
    pub async fn start_workers<C>(&self, ctx: QueueCtx, context: C, queues: Vec<String>) -> QueueResult<WorkerHandle>
    where
        C: Clone + Send + Sync + 'static,
    {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        
        let adapter_clone: QueueAdapter<dyn QueueBackend + Send + Sync> = QueueAdapter {
            backend: self.backend.clone() as Arc<dyn QueueBackend + Send + Sync>,
            codec_registry: self.codec_registry.clone(),
            job_registry: self.job_registry.clone(),
            observability: self.observability.clone(),
            config: self.config.clone(),
        };
        
        let worker = Worker {
            adapter: Arc::new(adapter_clone),
            ctx: ctx.clone(),
            context: Arc::new(context),
            queues,
            shutdown_rx: Some(shutdown_rx),
        };
        
        let join_handle = tokio::spawn(async move {
            worker.run().await
        });
        
        info!("Started worker for tenant: {}", ctx.tenant_id);
        
        Ok(WorkerHandle {
            shutdown_tx,
            join_handle,
        })
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

    /// Get configuration
    pub fn config(&self) -> &QueueConfig {
        &self.config
    }

    async fn create_execution_context<J: Job>(&self, _ctx: QueueCtx) -> QueueResult<J::Context> {
        Err(QueueError::Internal("Context creation not implemented for generic jobs".to_string()))
    }
}

impl<B: QueueBackend> Clone for QueueAdapter<B> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            codec_registry: self.codec_registry.clone(),
            job_registry: self.job_registry.clone(),
            observability: self.observability.clone(),
            config: self.config.clone(),
        }
    }
}

/// Worker for processing jobs from queues
struct Worker<C> {
    adapter: Arc<QueueAdapter<dyn QueueBackend + Send + Sync>>,
    ctx: QueueCtx,
    context: Arc<C>,
    queues: Vec<String>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl<C: Send + Sync + 'static> Worker<C> {
    /// Run the worker loop
    async fn run(mut self) -> QueueResult<()> {
        let mut shutdown_rx = self.shutdown_rx.take().unwrap();
        let queue_refs: Vec<&str> = self.queues.iter().map(|s| s.as_str()).collect();
        
        info!("Worker started for queues: {:?}", self.queues);
        
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    info!("Worker shutdown requested");
                    break;
                }
                
                result = self.process_next_job(&queue_refs) => {
                    match result {
                        Ok(processed) => {
                            if !processed {
                                // No jobs available, wait a bit
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                        Err(e) => {
                            error!("Error processing job: {}", e);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        }
        
        info!("Worker stopped");
        Ok(())
    }

    /// Process the next available job
    async fn process_next_job(&self, queues: &[&str]) -> QueueResult<bool> {
        // Dequeue next job
        let leased_job = match self.adapter.backend.dequeue(self.ctx.clone(), queues).await? {
            Some(job) => job,
            None => return Ok(false), // No jobs available
        };

        let job_id = leased_job.record.job_id.clone();
        let job_type = &leased_job.record.message.job_type;
        
        debug!("Processing job {} of type {}", job_id, job_type);

        // Get job registry
        let registry = self.adapter.job_registry.read().await;
        
        // Execute job through registry
        let result = registry.execute_job(
            &leased_job.record.message,
            self.context.clone(),
        ).await;

        match result {
            Ok(result_ref) => {
                // Job completed successfully
                self.adapter.backend.ack_complete(
                    self.ctx.clone(),
                    job_id.clone(),
                    leased_job.lease_token,
                    result_ref,
                ).await?;
                
                self.adapter.observability.record_job_completed(&self.ctx, &job_id, job_type).await;
                info!("Job {} completed successfully", job_id);
            }
            
            Err(job_error) => {
                // Job failed - determine if retryable
                let is_retryable = job_error.is_retryable();
                let retry_at = if is_retryable && leased_job.record.attempt < leased_job.record.message.max_retries {
                    Some(self.calculate_retry_time(leased_job.record.attempt))
                } else {
                    None
                };

                self.adapter.backend.ack_fail(
                    self.ctx.clone(),
                    job_id.clone(),
                    leased_job.lease_token,
                    job_error.to_string(),
                    retry_at,
                ).await?;

                if retry_at.is_some() {
                    self.adapter.observability.record_job_retrying(&self.ctx, &job_id, job_type).await;
                    warn!("Job {} failed, will retry: {}", job_id, job_error);
                } else {
                    self.adapter.observability.record_job_failed(&self.ctx, &job_id, job_type).await;
                    error!("Job {} failed permanently: {}", job_id, job_error);
                }
            }
        }

        Ok(true)
    }

    /// Calculate retry time with exponential backoff
    fn calculate_retry_time(&self, attempt: u32) -> chrono::DateTime<chrono::Utc> {
        let backoff_seconds = std::cmp::min(
            self.adapter.config.base_retry_backoff.as_secs() * (2_u64.pow(attempt.saturating_sub(1))),
            self.adapter.config.max_retry_backoff.as_secs(),
        );
        
        chrono::Utc::now() + chrono::Duration::seconds(backoff_seconds as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::{JobError, Job, JobPriority};
    use crate::backend::memory::MemoryBackend;

    #[derive(Clone)]
    struct TestContext {
        value: String,
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct TestJob {
        data: String,
    }

    #[async_trait]
    impl Job for TestJob {
        type Context = TestContext;
        type Result = String;

        const JOB_TYPE: &'static str = "test_job";
        const PRIORITY: crate::JobPriority = JobPriority::Normal;
        const MAX_RETRIES: u32 = 3;

        async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
            Ok(format!("Processed: {} with context: {}", self.data, ctx.value))
        }
    }

    #[tokio::test]
    async fn test_adapter_creation() {
        let backend = MemoryBackend::new();
        let adapter = QueueAdapter::new(backend);
        
        assert_eq!(adapter.config().max_workers, 10);
    }

    #[tokio::test]
    async fn test_job_registration() {
        let backend = MemoryBackend::new();
        let adapter = QueueAdapter::new(backend);
        
        let result = adapter.register_job::<TestJob>().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_enqueue_job() {
        let backend = MemoryBackend::new();
        let adapter = QueueAdapter::new(backend);
        
        // Register the job type first
        adapter.register_job::<TestJob>().await.unwrap();
        
        let ctx = QueueCtx::new("test_tenant".to_string());
        let job = TestJob { data: "test".to_string() };
        
        // Now that we have working codec implementation, this should succeed
        let result = adapter.enqueue(ctx, job).await;
        assert!(result.is_ok());
    }
}
