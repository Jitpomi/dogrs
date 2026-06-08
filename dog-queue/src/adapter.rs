use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, warn};

use crate::{
    backend::QueueBackend,
    codec::{CodecRegistry, EnqueueOptions},
    job::JobRegistry,
    observability::ObservabilityLayer,
    Job, JobId, QueueCtx, QueueError, QueueResult,
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
    /// How long a worker sleeps between dequeue polls when the queue is empty.
    /// Lower values reduce job latency at the cost of more backend round-trips.
    pub poll_interval: Duration,
    /// How long a worker backs off after an infrastructure error (e.g. backend
    /// unavailable) before retrying the dequeue loop.
    pub error_backoff: Duration,
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
            poll_interval: Duration::from_millis(100),
            error_backoff: Duration::from_secs(1),
        }
    }
}

/// Handle for managing the lifecycle of a worker pool.
///
/// Dropping this handle without calling `shutdown()` leaves the workers
/// running until the runtime shuts down.
pub struct WorkerHandle {
    shutdown_txs: Vec<oneshot::Sender<()>>,
    join_handles: Vec<JoinHandle<QueueResult<()>>>,
}

impl WorkerHandle {
    /// Gracefully signal all workers to stop and wait for them to finish.
    pub async fn shutdown(self) -> QueueResult<()> {
        // Signal every worker first so they can all drain concurrently.
        for tx in self.shutdown_txs {
            let _ = tx.send(());
        }
        // Then await each one.  Collect all errors rather than stopping at the first.
        let mut errors: Vec<String> = Vec::new();
        for handle in self.join_handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => errors.push(e.to_string()),
                Err(e) => errors.push(format!("Worker panicked: {e}")),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(QueueError::Internal(errors.join("; ")))
        }
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
impl QueueConfig {
    /// Validate that the configuration satisfies all required invariants.
    ///
    /// Returns an error if:
    /// - `max_workers` is 0 (no workers would ever start)
    /// - `base_retry_backoff` > `max_retry_backoff` (cap is below base; every retry uses max)
    pub fn validate(&self) -> QueueResult<()> {
        if self.max_workers == 0 {
            return Err(QueueError::Internal(
                "QueueConfig: max_workers must be >= 1 (0 workers would never process jobs)"
                    .to_string(),
            ));
        }
        if self.base_retry_backoff > self.max_retry_backoff {
            return Err(QueueError::Internal(format!(
                "QueueConfig: base_retry_backoff ({:?}) must be <= max_retry_backoff ({:?})",
                self.base_retry_backoff, self.max_retry_backoff,
            )));
        }
    Ok(())
    }
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

    /// Enqueue a job for immediate processing (runs now, in the job's default queue).
    ///
    /// For delayed scheduling or custom queue routing use [`Self::enqueue_opts`].
    #[instrument(skip(self, job), fields(job_type = J::JOB_TYPE, tenant_id = %ctx.tenant_id))]
    pub async fn enqueue<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<JobId> {
        self.enqueue_opts(ctx, job, EnqueueOptions::default()).await
    }

    /// Enqueue a job with caller-supplied options (queue name, delayed run_at).
    #[instrument(skip(self, job), fields(job_type = J::JOB_TYPE, tenant_id = %ctx.tenant_id))]
    pub async fn enqueue_opts<J: Job>(
        &self,
        ctx: QueueCtx,
        job: J,
        opts: EnqueueOptions,
    ) -> QueueResult<JobId> {
        // Encode job using codec registry
        let message = self.codec_registry.encode_job(&job, &ctx, opts)?;

        // Capture the real queue name before the message is moved into the backend.
        let queue_name = message.queue.clone();

        // Enqueue to backend
        let job_id = self.backend.enqueue(ctx.clone(), message).await?;

        // Record metrics — pass the real queue name, not a hardcoded default.
        self.observability
            .record_job_enqueued(&ctx, &job_id, J::JOB_TYPE, &queue_name)
            .await;

        info!("Enqueued job {} of type {}", job_id, J::JOB_TYPE);
        Ok(job_id)
    }

    /// Execute a job immediately, bypassing durable storage.
    ///
    /// **For development and testing only.** This path skips `enqueue`, `dequeue`,
    /// and `ack_complete`, so the job has no `JobId` and is invisible to the normal
    /// worker pipeline. In production always use `enqueue`.
    #[instrument(skip(self, job, execution_context), fields(job_type = J::JOB_TYPE, tenant_id = %ctx.tenant_id))]
    pub async fn execute_now<J: Job>(
        &self,
        ctx: QueueCtx,
        job: J,
        execution_context: J::Context,
    ) -> QueueResult<J::Result> {
        let _ = ctx; // kept for API symmetry with enqueue
        info!("Executing job immediately: {}", J::JOB_TYPE);

        // Execute with the configured lease duration as the timeout.
        // No observability recording: no real JobId exists (job was never enqueued)
        // and a phantom ID would produce uncorrelatable dashboard entries.
        tokio::time::timeout(self.config.lease_duration, job.execute(execution_context))
            .await
            .map_err(|_| QueueError::Internal("Job execution timeout".to_string()))?
            .map_err(QueueError::JobFailed)
    }

    /// Erase the concrete backend type to `dyn QueueBackend + Send + Sync`.
    ///
    /// Used internally by `start_workers` to share one type-erased adapter
    /// across all spawned workers.  Centralising the field copy here means a
    /// compiler error (missing field) if `QueueAdapter` ever gains a new field.
    fn into_dyn_shared(&self) -> QueueAdapter<dyn QueueBackend + Send + Sync>
    where
        B: 'static,
    {
        QueueAdapter {
            backend: self.backend.clone() as Arc<dyn QueueBackend + Send + Sync>,
            codec_registry: self.codec_registry.clone(),
            job_registry: self.job_registry.clone(),
            observability: self.observability.clone(),
            config: self.config.clone(),
        }
    }

    /// Start a pool of `config.max_workers` concurrent workers.
    ///
    /// All workers share the same `Arc`-wrapped state (backend, registry,
    /// observability) and are coordinated by the returned [`WorkerHandle`].
    /// Call [`WorkerHandle::shutdown`] to gracefully stop them all.
    #[instrument(skip(self, context), fields(tenant_id = %ctx.tenant_id, queues = ?queues))]
    pub async fn start_workers<C>(
        &self,
        ctx: QueueCtx,
        context: C,
        queues: Vec<String>,
    ) -> QueueResult<WorkerHandle>
    where
        C: Clone + Send + Sync + 'static,
    {
        // Enforce config invariants before we start spawning.
        self.config.validate()?;

        let worker_count = self.config.max_workers;
        let mut shutdown_txs = Vec::with_capacity(worker_count);
        let mut join_handles = Vec::with_capacity(worker_count);

        // Build one type-erased adapter shared across all workers.
        let dyn_adapter = Arc::new(self.into_dyn_shared());

        for _ in 0..worker_count {
            let (shutdown_tx, shutdown_rx) = oneshot::channel();

            let worker = Worker {
                adapter: dyn_adapter.clone(),
                ctx: ctx.clone(),
                context: Arc::new(context.clone()),
                queues: queues.clone(),
            };

            let join_handle = tokio::spawn(async move { worker.run(shutdown_rx).await });

            shutdown_txs.push(shutdown_tx);
            join_handles.push(join_handle);
        }

        info!(
            "Started {} worker(s) for tenant: {}",
            worker_count, ctx.tenant_id
        );

        Ok(WorkerHandle {
            shutdown_txs,
            join_handles,
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
    // NOTE: shutdown_rx is NOT stored here — it is passed directly to run()
    // so that process_next_job can borrow self without a partial-move conflict.
}

impl<C: Send + Sync + 'static> Worker<C> {
    /// Run the worker loop
    async fn run(self, mut shutdown_rx: oneshot::Receiver<()>) -> QueueResult<()> {
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
                                // No jobs available — sleep for the configured poll interval
                                // before trying again. Tunable via QueueConfig::poll_interval.
                                tokio::time::sleep(self.adapter.config.poll_interval).await;
                            }
                        }
                        Err(e) => {
                            error!("Error processing job: {}", e);
                            // Infrastructure error — back off before retrying to avoid
                            // hammering a degraded backend. Tunable via QueueConfig::error_backoff.
                            tokio::time::sleep(self.adapter.config.error_backoff).await;
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
        let leased_job = match self
            .adapter
            .backend
            .dequeue(self.ctx.clone(), queues)
            .await?
        {
            Some(job) => job,
            None => return Ok(false), // No jobs available
        };

        let job_id = leased_job.record.job_id.clone();
        let job_type = &leased_job.record.message.job_type;

        debug!("Processing job {} of type {}", job_id, job_type);

        // Clone the handler under the registry lock, then release the lock before
        // executing. This prevents long-running jobs from blocking register_job()
        // which needs the write lock.
        let handler = {
            let registry = self.adapter.job_registry.read().await;
            registry.get_handler(job_type).ok_or_else(|| {
                QueueError::Internal(format!("No handler registered for job type '{job_type}'"))
            })?
        }; // read lock released here

        // Spawn a heartbeat task that extends the lease every `heartbeat_interval`
        // while execute() runs.  Without this, any job that takes longer than
        // `lease_duration` (default 5 min) is reclaimed by the reaper and re-executed
        // by another worker while the original is still running — silent double-execution
        // for non-idempotent jobs.
        //
        // The heartbeat is aborted as soon as execute() returns so it cannot fire
        // between execute() completing and ack_complete/ack_fail being called.
        // If the job is canceled or the lease token is invalidated, heartbeat_extend
        // returns an error; the heartbeat loop exits and the main worker's
        // ack_complete will surface the JobCanceled / InvalidLeaseToken error.
        let hb_backend = self.adapter.backend.clone();
        let hb_ctx = self.ctx.clone();
        let hb_job_id = job_id.clone();
        let hb_token = leased_job.lease_token.clone();
        let hb_interval = self.adapter.config.heartbeat_interval;

        let heartbeat_handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(hb_interval).await;
                match hb_backend
                    .heartbeat_extend(
                        hb_ctx.clone(),
                        hb_job_id.clone(),
                        hb_token.clone(),
                        hb_interval,
                    )
                    .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        warn!(
                            "Heartbeat extension failed for job {} (stopping heartbeat): {}",
                            hb_job_id, e
                        );
                        break;
                    }
                }
            }
        });

        let result = handler
            .execute(&leased_job.record.message, self.context.clone())
            .await;

        // Job finished — stop the heartbeat unconditionally.
        heartbeat_handle.abort();

        match result {
            Ok(result_ref) => {
                // Job completed successfully
                self.adapter
                    .backend
                    .ack_complete(
                        self.ctx.clone(),
                        job_id.clone(),
                        leased_job.lease_token,
                        result_ref,
                    )
                    .await?;

                self.adapter
                    .observability
                    .record_job_completed(&self.ctx, &job_id, job_type)
                    .await;
                info!("Job {} completed successfully", job_id);
            }

            Err(job_error) => {
                // Job failed - determine if retryable
                let is_retryable = job_error.is_retryable();
                // MAX_RETRIES is the number of *extra* retries after the initial attempt
                // (same convention as Bull, Sidekiq, Celery). Use <= so that
                // attempt == MAX_RETRIES still schedules one more retry.
                let retry_at = if is_retryable
                    && leased_job.record.attempt <= leased_job.record.message.max_retries
                {
                    Some(self.calculate_retry_time(leased_job.record.attempt))
                } else {
                    None
                };

                // Capture error string once; used by ack_fail AND observability.
                let error_str = job_error.to_string();

                self.adapter
                    .backend
                    .ack_fail(
                        self.ctx.clone(),
                        job_id.clone(),
                        leased_job.lease_token,
                        error_str.clone(),
                        retry_at,
                    )
                    .await?;

                if let Some(retry_at_time) = retry_at {
                    self.adapter
                        .observability
                        .record_job_retrying(
                            &self.ctx,
                            &job_id,
                            job_type,
                            &error_str,
                            retry_at_time,
                        )
                        .await;
                    warn!("Job {} failed, will retry: {}", job_id, error_str);
                } else {
                    self.adapter
                        .observability
                        .record_job_failed(&self.ctx, &job_id, job_type, &error_str)
                        .await;
                    error!("Job {} failed permanently: {}", job_id, error_str);
                }
            }
        }

        Ok(true)
    }

    /// Calculate retry time with exponential backoff
    fn calculate_retry_time(&self, attempt: u32) -> chrono::DateTime<chrono::Utc> {
        let backoff_seconds = std::cmp::min(
            self.adapter.config.base_retry_backoff.as_secs()
                * (2_u64.pow(attempt.saturating_sub(1))),
            self.adapter.config.max_retry_backoff.as_secs(),
        );

        chrono::Utc::now() + chrono::Duration::seconds(backoff_seconds as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::memory::MemoryBackend;
    use crate::{Job, JobError, JobPriority};
    use async_trait::async_trait;

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
            Ok(format!(
                "Processed: {} with context: {}",
                self.data, ctx.value
            ))
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
        let job = TestJob {
            data: "test".to_string(),
        };

        // Now that we have working codec implementation, this should succeed
        let result = adapter.enqueue(ctx, job).await;
        assert!(result.is_ok());
    }
}
