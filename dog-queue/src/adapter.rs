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
    /// How long a worker may remain idle (no jobs available) before it
    /// self-terminates. Keeps the pool size proportional to actual load.
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
    /// Random jitter added to `poll_interval` per-worker to stagger dequeue
    /// requests across the pool.
    ///
    /// Each worker adds a random delay in `[0, poll_jitter]` before sleeping
    /// `poll_interval`. With `max_workers = 10`, `poll_interval = 100ms`, and
    /// `poll_jitter = 10ms`, workers spread their polls over a 10 ms window
    /// instead of all hitting the backend simultaneously. Critical for
    /// Redis/Postgres backends — without jitter, every idle worker issues a
    /// dequeue request at the same instant, creating a thundering herd.
    ///
    /// Must be `<= poll_interval` (enforced by [`QueueConfig::validate`]).
    /// Defaults to `10ms`, which is 10% of the default 100 ms poll interval.
    pub poll_jitter: Duration,
    /// How long a worker backs off after an infrastructure error (e.g. backend
    /// unavailable) before retrying the dequeue loop.
    pub error_backoff: Duration,
    /// Hard timeout for `execute_now`. `None` means no timeout is applied.
    ///
    /// This is intentionally separate from `lease_duration` — `execute_now`
    /// has no backend lease and should not be constrained by lease recycling
    /// settings. Set to `Some(Duration)` when you need a safety cap on direct
    /// execution (e.g. in tests or one-off CLI invocations).
    pub execute_timeout: Option<Duration>,

    /// Maximum encoded payload size in bytes.
    ///
    /// `None` (the default) applies no limit.
    ///
    /// When set, [`QueueAdapter::enqueue_opts`] returns
    /// [`QueueError::PayloadTooLarge`] if the encoded payload exceeds this
    /// threshold, before the job reaches the backend. Set this to protect
    /// downstream systems (database column width, message-broker limits, etc.)
    /// from oversized payloads at the enqueue boundary.
    pub max_payload_size: Option<usize>,
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
            poll_jitter: Duration::from_millis(10), // 10% of poll_interval
            error_backoff: Duration::from_secs(1),
            execute_timeout: None, // no timeout by default
            max_payload_size: None, // no limit by default
        }
    }
}

impl QueueConfig {
    /// Validate that the configuration satisfies all required invariants.
    ///
    /// Returns an error if:
    /// - `max_workers` is 0 (no workers would ever start)
    /// - `base_retry_backoff` > `max_retry_backoff` (cap is below base; every retry uses max)
    /// - `heartbeat_interval` >= `lease_duration` (first heartbeat arrives after lease has already
    ///   expired; the reaper reclaims the job mid-execution, causing silent double-execution)
    /// - `poll_interval` is zero (busy-wait spin loop against the backend)
    /// - `error_backoff` is zero (immediate tight retry loop after backend errors)
    /// - `poll_jitter` > `poll_interval` (jitter larger than the base interval is incoherent)
    pub fn validate(&self) -> QueueResult<()> {
        if self.max_workers == 0 {
            return Err(QueueError::InvalidConfig(
                "max_workers must be >= 1 (0 workers would never process jobs)"
                    .to_string(),
            ));
        }
        if self.base_retry_backoff > self.max_retry_backoff {
            return Err(QueueError::InvalidConfig(format!(
                "base_retry_backoff ({:?}) must be <= max_retry_backoff ({:?})",
                self.base_retry_backoff, self.max_retry_backoff,
            )));
        }
        if self.heartbeat_interval >= self.lease_duration {
            return Err(QueueError::InvalidConfig(format!(
                "heartbeat_interval ({:?}) must be < lease_duration ({:?}) — \
                 otherwise the lease expires before the first heartbeat fires and \
                 the reaper reclaims the job mid-execution, causing double-execution",
                self.heartbeat_interval, self.lease_duration,
            )));
        }
        if self.poll_interval.is_zero() {
            return Err(QueueError::InvalidConfig(
                "poll_interval must be > 0 — a zero interval causes a busy-wait \
                 spin loop that hammers the backend with continuous dequeue requests"
                    .to_string(),
            ));
        }
        if self.error_backoff.is_zero() {
            return Err(QueueError::InvalidConfig(
                "error_backoff must be > 0 — a zero interval causes a tight retry \
                 loop immediately after backend errors with no recovery window"
                    .to_string(),
            ));
        }
        if self.poll_jitter > self.poll_interval {
            return Err(QueueError::InvalidConfig(format!(
                "poll_jitter ({:?}) must be <= poll_interval ({:?}) — \
                 a jitter larger than the base interval defeats the poll cadence",
                self.poll_jitter, self.poll_interval,
            )));
        }
        Ok(())
    }
}

/// Handle for managing the lifecycle of a worker pool.
///
/// Dropping this handle without calling `shutdown()` leaves the workers
/// running until the runtime shuts down.
pub struct WorkerHandle {
    shutdown_txs: Vec<oneshot::Sender<()>>,
    join_handles: Vec<JoinHandle<QueueResult<()>>>,
    /// Shutdown signal for the integrated reaper task (if one was spawned).
    reaper_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Join handle for the integrated reaper task.
    reaper_handle: Option<JoinHandle<QueueResult<()>>>,
}

impl WorkerHandle {
    /// Gracefully signal all workers and the integrated reaper to stop, then wait
    /// for them all to finish.
    pub async fn shutdown(self) -> QueueResult<()> {
        // Signal every worker and the reaper first so they can all drain concurrently.
        for tx in self.shutdown_txs {
            let _ = tx.send(());
        }
        if let Some(tx) = self.reaper_shutdown_tx {
            let _ = tx.send(());
        }
        // Await each handle, collecting all errors rather than stopping at the first.
        // Log each error individually so operators have structured granularity
        // (worker panic vs. reaper error) before the errors are merged.
        let mut errors: Vec<String> = Vec::new();
        for handle in self.join_handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    error!("Worker shutdown error: {e}");
                    errors.push(e.to_string());
                }
                Err(e) => {
                    error!("Worker panicked during shutdown: {e}");
                    errors.push(format!("Worker panicked: {e}"));
                }
            }
        }
        if let Some(handle) = self.reaper_handle {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    error!("Reaper shutdown error: {e}");
                    errors.push(format!("Reaper: {e}"));
                }
                Err(e) => {
                    error!("Reaper panicked during shutdown: {e}");
                    errors.push(format!("Reaper panicked: {e}"));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(QueueError::Internal(format!(
                "{} shutdown error(s): {}",
                errors.len(),
                errors.join("; ")
            )))
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

    /// Create adapter with custom configuration.
    ///
    /// # Panics
    ///
    /// Panics if `config` violates an invariant checked by [`QueueConfig::validate`]
    /// (e.g. `max_workers == 0` or `base_retry_backoff > max_retry_backoff`).
    /// Use [`Self::try_with_config`] when you need a fallible constructor.
    pub fn with_config(backend: B, config: QueueConfig) -> Self {
        config
            .validate()
            .expect("QueueConfig is invalid — see QueueConfig::validate() for details");
        Self {
            backend: Arc::new(backend),
            codec_registry: Arc::new(CodecRegistry::new()),
            job_registry: Arc::new(RwLock::new(JobRegistry::new())),
            observability: Arc::new(ObservabilityLayer::new()),
            config,
        }
    }

    /// Create adapter with custom configuration, returning an error on invalid config.
    ///
    /// Prefer this over [`Self::with_config`] when the config is derived from
    /// runtime inputs (environment variables, config files) rather than constants.
    pub fn try_with_config(backend: B, config: QueueConfig) -> QueueResult<Self> {
        config.validate()?;
        Ok(Self {
            backend: Arc::new(backend),
            codec_registry: Arc::new(CodecRegistry::new()),
            job_registry: Arc::new(RwLock::new(JobRegistry::new())),
            observability: Arc::new(ObservabilityLayer::new()),
            config,
        })
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
        let message = self.codec_registry.encode_job(&job, opts)?;

        // Enforce the configured payload size limit.
        // This check is at the adapter (not codec) layer because encode_job
        // has no access to QueueConfig — the adapter is the single owner of config.
        if let Some(max) = self.config.max_payload_size {
            let size = message.payload_bytes.len();
            if size > max {
                return Err(QueueError::PayloadTooLarge { size, max });
            }
        }

        // Capture the real queue name before the message is moved into the backend.
        let queue_name = message.queue.clone();

        // Enqueue to backend
        let job_id = self.backend.enqueue(ctx.clone(), message).await?;

        // Record metrics — pass the real queue name, not a hardcoded default.
        self.observability
            .record_job_enqueued(&ctx, &job_id, J::JOB_TYPE, &queue_name);

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

        // Execute with an optional hard timeout.
        // `execute_timeout` is distinct from `lease_duration`: the lease controls
        // backend claim recycling, while this timeout guards the direct execution
        // path which has no lease, no reaper, and no heartbeat.
        let execute_fut = job.execute(execution_context);
        match self.config.execute_timeout {
            Some(limit) => tokio::time::timeout(limit, execute_fut)
                .await
                .map_err(|_| QueueError::Timeout(limit))?
                .map_err(QueueError::JobFailed),
            None => execute_fut.await.map_err(QueueError::JobFailed),
        }
    }

    /// Cancel a job by ID.
    ///
    /// Returns `true` if the job was found and successfully canceled, `false`
    /// if it was already in a terminal state (`Completed`, `Failed`, or already
    /// `Canceled`).  Cancel-wins semantics apply: if a worker is currently
    /// executing the job, the next `ack_complete` call will return
    /// [`QueueError::JobCanceled`] and the result will be discarded.
    ///
    /// Cancellation is recorded in the observability layer so `jobs_canceled`
    /// reflects all cancellations that go through this adapter method.
    #[instrument(skip(self), fields(tenant_id = %ctx.tenant_id, job_id = %job_id))]
    pub async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
        // Resolve job_type BEFORE canceling for observability — best-effort only.
        //
        // Using `.ok()` instead of `.unwrap_or_default()`: if `get_record()` fails
        // (job already terminal, backend error), we get `None` and skip the metrics
        // increment rather than recording `job_type = ""`, which would create a phantom
        // `""` key in `LiveMetrics::per_type` that contaminates all job-type snapshots.
        let job_type = self
            .backend
            .get_record(ctx.clone(), job_id.clone())
            .await
            .ok()
            .map(|r| r.message.job_type);

        let canceled = self.backend.cancel(ctx.clone(), job_id.clone()).await?;
        if canceled {
            // Only record metrics when we know the job type — never emit a blank key.
            if let Some(ref jt) = job_type {
                self.observability.record_job_canceled(&ctx, &job_id, jt);
            }
            info!("Canceled job {}", job_id);
        }
        Ok(canceled)
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
        // config.validate() is enforced at construction (with_config panics,
        // try_with_config returns an error, new() uses a hard-coded valid default).
        // Re-validating here would be unreachable dead code.

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

        // Spawn the integrated reaper task at lease_duration / 2 intervals.
        //
        // Correlating the reaper interval with lease_duration satisfies the invariant:
        //   reaper_interval < lease_duration
        // so the reaper fires at least once within every lease window.  Backends that
        // manage expiry externally (Redis EXPIRE, Postgres pg_cron) return Ok(vec!) from
        // reclaim_expired_leases() and pay only a trivial polling cost.
        let (reaper_shutdown_tx, mut reaper_shutdown_rx) = oneshot::channel::<()>();
        let reaper_backend = dyn_adapter.backend.clone();
        // Clone the observability layer so the reaper can record per-type metrics for
        // each reclaimed lease.  Without this, lease-expiry failures and retries are
        // invisible to jobs_failed / jobs_retried counters and success_rate().
        let reaper_observability = dyn_adapter.observability.clone();
        let reaper_interval = {
            let half_secs = self.config.lease_duration.as_secs() / 2;
            std::time::Duration::from_secs(half_secs.max(1))
        };
        let reaper_handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(reaper_interval);
            // Delay mode: if the reaper cycle takes longer than the interval
            // (e.g. under high load), skip ticks rather than queuing them up.
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    _ = &mut reaper_shutdown_rx => {
                        info!("Integrated reaper shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        match reaper_backend.reclaim_expired_leases().await {
                            Ok(ref outcomes) if outcomes.is_empty() => {}
                            Ok(outcomes) => {
                                info!("Reaper: reclaimed {} expired lease(s)", outcomes.len());
                                // Record per-type observability metrics for each reclaimed
                                // lease.  Previously the reaper returned only a count, so
                                // all reaper-reclaimed failures and retries were permanently
                                // invisible to jobs_failed / jobs_retried / success_rate().
                                for outcome in &outcomes {
                                    let ctx = QueueCtx::new(outcome.tenant_id.clone());
                                    if outcome.permanently_failed {
                                        reaper_observability.record_job_failed(
                                            &ctx,
                                            &outcome.job_id,
                                            &outcome.job_type,
                                            "Lease expired — max retries exceeded",
                                        );
                                    } else if let Some(retry_at) = outcome.retry_at {
                                        reaper_observability.record_job_retrying(
                                            &ctx,
                                            &outcome.job_id,
                                            &outcome.job_type,
                                            "Lease expired — re-queued for retry",
                                            retry_at,
                                        );
                                    }
                                }
                            }
                            Err(e) => warn!("Reaper error during reclaim: {e}"),
                        }
                    }
                }
            }
            Ok(())
        });

        Ok(WorkerHandle {
            shutdown_txs,
            join_handles,
            reaper_shutdown_tx: Some(reaper_shutdown_tx),
            reaper_handle: Some(reaper_handle),
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

    /// Retrieve the stored result of a completed job, deserialized to `J::Result`.
    ///
    /// Returns `Ok(Some(result))` if the job has completed and its result was
    /// stored.  Returns `Ok(None)` if the job completed but produced no result
    /// (i.e. the handler returned `Ok(None)` or the backend stored an empty value).
    ///
    /// # Errors
    ///
    /// - [`QueueError::JobNotFound`] — the job ID does not exist.
    /// - [`QueueError::BackendUnsupported`] — the backend does not implement
    ///   [`QueueBackend::get_record`].  For `MemoryBackend` this never happens.
    /// - [`QueueError::SerializationError`] — the stored result bytes cannot be
    ///   deserialized to `J::Result` (e.g. a schema migration happened between
    ///   enqueue and retrieval).
    pub async fn get_result<J: Job>(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
    ) -> QueueResult<Option<J::Result>> {
        let record = self.backend.get_record(ctx, job_id).await?;

        let result_str = match record.result {
            Some(ref s) if !s.is_empty() => s,
            _ => return Ok(None),
        };

        let result: J::Result = serde_json::from_str(result_str)
            .map_err(|e| QueueError::SerializationError(format!(
                "Failed to deserialize stored result for job type '{}': {e}",
                J::JOB_TYPE
            )))?;

        Ok(Some(result))
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

/// RAII guard that aborts the wrapped task when dropped.
///
/// When `tokio::select!` cancels a future that owns a `JoinHandle`, Tokio
/// **detaches** (does not abort) the spawned task.  Wrapping the heartbeat
/// handle in `AbortOnDrop` ensures the task is always terminated when
/// `process_next_job` is cancelled by a shutdown signal, preventing orphaned
/// heartbeat tasks that extend leases indefinitely after the worker exits.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
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
    /// Run the worker loop, terminating on shutdown signal or after the
    /// configured idle timeout elapses with no jobs available.
    async fn run(self, mut shutdown_rx: oneshot::Receiver<()>) -> QueueResult<()> {
        let queue_refs: Vec<&str> = self.queues.iter().map(|s| s.as_str()).collect();

        info!("Worker started for queues: {:?}", self.queues);

        // Track consecutive idle time so the worker can self-terminate.
        // Reset to `None` whenever a job is successfully processed.
        let mut idle_since: Option<std::time::Instant> = None;

        // Track consecutive infrastructure errors to apply exponential backoff
        // and suppress log spam during prolonged backend outages.
        //
        // With flat error_backoff (1 s) and max_workers (10), a 3-hour outage would
        // produce 10 × 3 × 3600 = 108,000 error! log lines. Exponential escalation
        // caps at 30 s and power-of-two log sampling keeps ops informed without
        // overwhelming log ingestion pipelines.
        let mut consecutive_errors: u32 = 0;

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    info!("Worker shutdown requested");
                    break;
                }

                result = self.process_next_job(&queue_refs) => {
                    match result {
                        Ok(true) => {
                            // A job ran — reset both the idle clock and error counter.
                            if consecutive_errors > 0 {
                                info!(
                                    "Backend recovered after {} consecutive error(s)",
                                    consecutive_errors
                                );
                                consecutive_errors = 0;
                            }
                            idle_since = None;
                        }
                        Ok(false) => {
                            // No jobs available — reset error counter, track idle duration.
                            if consecutive_errors > 0 {
                                info!(
                                    "Backend recovered after {} consecutive error(s)",
                                    consecutive_errors
                                );
                                consecutive_errors = 0;
                            }
                            let idle_start = *idle_since.get_or_insert_with(std::time::Instant::now);
                            if idle_start.elapsed() >= self.adapter.config.worker_idle_timeout {
                                info!(
                                    "Worker idle for {:?}, shutting down",
                                    self.adapter.config.worker_idle_timeout
                                );
                                break;
                            }
                            // Sleep before next poll, adding a random jitter in
                            // [0, poll_jitter] to stagger workers across the pool.
                            // Without jitter, all workers wake and issue dequeue
                            // requests at the same instant — a thundering herd for
                            // Redis/Postgres backends.
                            let jitter_nanos = if self.adapter.config.poll_jitter.is_zero() {
                                0u64
                            } else {
                                // rand::random_range is the top-level free function
                                // in rand 0.10 — no Rng trait import required.
                                rand::random_range(
                                    0u64..=self.adapter.config.poll_jitter.as_nanos() as u64
                                )
                            };
                            let sleep_duration = self.adapter.config.poll_interval
                                + Duration::from_nanos(jitter_nanos);
                            tokio::time::sleep(sleep_duration).await;
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            // Log every first error and subsequent powers-of-two to stay
                            // informed without flooding log ingestion during long outages.
                            // Pattern: error at 1, warn at 2, 4, 8, 16, … → silences
                            // intermediate lines while preserving a clear escalation trail.
                            if consecutive_errors == 1 {
                                error!(
                                    "Backend error (will back off exponentially): {}", e
                                );
                            } else if consecutive_errors.is_power_of_two() {
                                warn!(
                                    "Backend still unavailable after {} error(s): {}",
                                    consecutive_errors, e
                                );
                            }
                            // Exponential backoff capped at 30s:
                            //   error #1 → 1s, #2 → 2s, #3 → 4s, #4 → 8s,
                            //   #5 → 16s, #6+ → 30s (cap).
                            // error_backoff (default 1s) is the base; min() caps at 30s.
                            // Using saturating_pow to prevent overflow on very long outages.
                            let exponent = consecutive_errors.saturating_sub(1).min(5);
                            let backoff = self
                                .adapter
                                .config
                                .error_backoff
                                .saturating_mul(2u32.saturating_pow(exponent))
                                .min(Duration::from_secs(30));
                            // Reset idle_since: distinguish degraded backend (worker is
                            // active, just failing) from empty queue (no jobs to process).
                            // Without this reset, an outage longer than worker_idle_timeout
                            // self-terminates all workers exactly when recovery throughput
                            // is most needed.
                            idle_since = None;
                            tokio::time::sleep(backoff).await;
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

        let heartbeat_handle = AbortOnDrop(tokio::spawn(async move {
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
        }));

        // Decode the payload through the registered codec before handing it to the handler.
        // `encode_bytes` was called at enqueue time; `decode_bytes` must be called here to
        // reverse any transformation (compression, encryption, alternate wire format).
        // For the JSON passthrough codec this is a no-op validation; for real codecs it is
        // mandatory — without this call the handler receives still-encoded bytes and
        // serde_json::from_slice silently produces a Permanent deserialization error.
        //
        // A decode failure is DETERMINISTIC — retrying will never fix a corrupt payload or
        // codec mismatch. We therefore immediately and permanently fail the job (ack_fail with
        // retry_at = None) rather than propagating `?` and leaving the job stranded in
        // Processing until the reaper expires the lease, re-queues it, and the next worker
        // burns another attempt on the same unfixable error.
        let decoded_bytes = match self
            .adapter
            .codec_registry
            .decode_job_payload(&leased_job.record.message)
        {
            Ok(b) => b,
            Err(e) => {
                let error_str =
                    format!("Codec decode failed (permanent — payload is corrupt or codec mismatch): {e}");
                error!("Job {} permanently failed: {}", job_id, error_str);

                // AbortOnDrop will abort the heartbeat task as it goes out of scope;
                // drop explicitly here to abort BEFORE calling ack_fail.
                drop(heartbeat_handle);

                // Permanently fail the job so it leaves Processing immediately.
                // Ignore ack_fail errors here — we cannot do anything useful with
                // them and the job will be reclaimed by the reaper at worst.
                let _ = self
                    .adapter
                    .backend
                    .ack_fail(
                        self.ctx.clone(),
                        job_id.clone(),
                        leased_job.lease_token,
                        error_str.clone(),
                        None, // retry_at = None → permanent failure
                    )
                    .await;

                self.adapter
                    .observability
                    .record_job_failed(&self.ctx, &job_id, job_type, &error_str);

                // Return Ok(true) — we did process a job (it permanently failed).
                // Returning Ok(false) would trigger the idle timer for an empty queue;
                // Err would trigger the error backoff; neither is correct here.
                return Ok(true);
            }
        };
        let mut decoded_message = leased_job.record.message.clone();
        decoded_message.payload_bytes = decoded_bytes;

        // Time the execute() call for performance metrics.
        // The elapsed duration is recorded after the drop of the heartbeat handle
        // so that heartbeat teardown overhead is not counted as job execution time.
        let execute_start = std::time::Instant::now();
        let result = handler
            .execute(&decoded_message, self.context.clone())
            .await;
        let execute_elapsed = execute_start.elapsed();

        // Job finished — drop the AbortOnDrop guard, which aborts the heartbeat task.
        drop(heartbeat_handle);

        // Record execution timing — this is the first caller of record_execution_time;
        // previously the PerformanceMetrics ring buffer was permanently empty.
        self.adapter
            .observability
            .metrics()
            .record_execution_time(job_type, execute_elapsed);

        match result {
            Ok(result_ref) => {
                // Job completed successfully — ack with the backend.
                // Handle terminal-state races explicitly rather than propagating with `?`:
                //   JobCanceled        — cancel arrived after execute() started; cancel-wins.
                //   JobAlreadyTerminal — reaper reclaimed between execute() and ack_complete();
                //                        the reaper has already re-queued the job, so we must
                //                        NOT double-ack or the result could be applied twice.
                match self
                    .adapter
                    .backend
                    .ack_complete(
                        self.ctx.clone(),
                        job_id.clone(),
                        leased_job.lease_token,
                        result_ref,
                    )
                    .await
                {
                    Ok(()) => {
                        self.adapter
                            .observability
                            .record_job_completed(&self.ctx, &job_id, job_type);
                        info!("Job {} completed successfully", job_id);
                    }
                    Err(QueueError::JobCanceled) => {
                        // Cancel-wins: a cancel request arrived after execute() started.
                        //
                        // record_job_canceled() was ALREADY called by QueueAdapter::cancel()
                        // when it successfully set the backend status to Canceled.
                        // Do NOT call any observability method here — that would double-count
                        // the event, inflating both jobs_canceled and jobs_failed for a single
                        // lifecycle event and contaminating success_rate() calculations.
                        warn!(
                            "Job {} completed execution but was canceled mid-flight — \
                             result discarded (cancel-wins); metrics already recorded by cancel()",
                            job_id
                        );
                    }
                    Err(QueueError::JobAlreadyTerminal) => {
                        // The reaper reclaimed the job between execute() completing and
                        // ack_complete() being called.  The reaper has already re-queued it;
                        // logging only — do NOT re-execute or alter state.
                        warn!(
                            "Job {} ack_complete: already terminal (reaper race?) — \
                             result discarded, job may be re-executed",
                            job_id
                        );
                    }
                    Err(e) => return Err(e), // Genuine infrastructure error — propagate.
                }
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
                        );
                    warn!("Job {} failed, will retry: {}", job_id, error_str);
                } else {
                    self.adapter
                        .observability
                        .record_job_failed(&self.ctx, &job_id, job_type, &error_str);
                    error!("Job {} failed permanently: {}", job_id, error_str);
                }
            }
        }

        Ok(true)
    }

    /// Calculate retry time using full-jitter exponential backoff.
    ///
    /// "Full jitter" (AWS recommendation): instead of `sleep = clamp(2^attempt * base, cap)`,
    /// pick uniformly from `[0, clamp(2^attempt * base, cap)]`. This desynchronises
    /// concurrent retriers that all failed at the same instant — preventing the thundering
    /// herd that pure exponential backoff causes on mass failures.
    fn calculate_retry_time(&self, attempt: u32) -> chrono::DateTime<chrono::Utc> {
        let cap = self.adapter.config.max_retry_backoff.as_secs();
        let base = self
            .adapter
            .config
            .base_retry_backoff
            .as_secs()
            .saturating_mul(2_u64.pow(attempt.saturating_sub(1)));
        let ceiling = base.min(cap);

        // Uniform sample in [0, ceiling] — each retrier picks a different slot.
        // rand::random_range is the rand 0.10 top-level API used throughout this file;
        // consistent with the poll_jitter sampling below. Inclusive upper bound
        // matches the documented semantics ("pick uniformly from [0, cap]").
        let jitter_secs = if ceiling > 0 {
            rand::random_range(0u64..=ceiling)
        } else {
            0
        };

        chrono::Utc::now() + chrono::Duration::seconds(jitter_secs as i64)
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

        let ctx = QueueCtx::new("test_tenant");
        let job = TestJob {
            data: "test".to_string(),
        };

        // Now that we have working codec implementation, this should succeed
        let result = adapter.enqueue(ctx, job).await;
        assert!(result.is_ok());
    }
}
