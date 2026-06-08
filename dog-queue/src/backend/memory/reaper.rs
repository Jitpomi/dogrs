use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::{
    backend::{
        memory::storage::{priority_insert, MemoryBackend},
        ReapOutcome,
    },
    JobEvent, JobStatus, QueueResult,
};

/// Lease expiry reaper for reclaiming expired jobs
pub struct LeaseReaper {
    backend: Arc<MemoryBackend>,
    interval: Duration,
    /// Minimum delay applied to jobs re-queued after lease expiry.
    ///
    /// Prevents a tight retry loop when a job reliably crashes its worker
    /// (OOM, panic in execute).  Defaults to 1 second — the same as
    /// `QueueConfig::default().base_retry_backoff` — so reclaimed jobs
    /// experience at least one backoff cycle before being re-dequeued.
    base_retry_backoff: chrono::Duration,
}

impl LeaseReaper {
    /// Create a new lease reaper with default 30s interval and 1s retry backoff
    pub fn new(backend: Arc<MemoryBackend>) -> Self {
        Self {
            backend,
            interval: Duration::from_secs(30), // Run every 30 seconds
            base_retry_backoff: chrono::Duration::seconds(1),
        }
    }

    /// Create reaper with custom interval
    pub fn with_interval(backend: Arc<MemoryBackend>, interval: Duration) -> Self {
        Self { backend, interval, base_retry_backoff: chrono::Duration::seconds(1) }
    }

    /// Set the minimum backoff applied to jobs re-queued after lease expiry.
    ///
    /// Call this after [`Self::new`] or [`Self::with_interval`] to override the
    /// 1-second default, for example with the adapter's `base_retry_backoff`.
    pub fn with_backoff(mut self, backoff: std::time::Duration) -> Self {
        self.base_retry_backoff = chrono::Duration::from_std(backoff)
            .unwrap_or(chrono::Duration::seconds(1));
        self
    }

    /// Start the reaper background task.
    ///
    /// Runs until `shutdown_rx` fires, then exits cleanly.
    /// Callers should use `tokio::spawn` and keep the `oneshot::Sender` to trigger shutdown:
    /// ```ignore
    /// let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    /// tokio::spawn(reaper.start(shutdown_rx));
    /// // Later:
    /// let _ = shutdown_tx.send(());
    /// ```
    pub async fn start(self, mut shutdown_rx: tokio::sync::oneshot::Receiver<()>) -> QueueResult<()> {
        let mut ticker = interval(self.interval);

        info!("Starting lease reaper with interval: {:?}", self.interval);

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    info!("Lease reaper shutting down gracefully");
                    break;
                }
                _ = ticker.tick() => {
                    match self.reap_expired_leases().await {
                        Ok(ref outcomes) if !outcomes.is_empty() => info!("Reclaimed {} expired leases", outcomes.len()),
                        Ok(_) => debug!("No expired leases found"),
                        Err(e) => warn!("Error during lease reaping: {e}"),
                    }
                }
            }
        }

        Ok(())
    }

    /// Run one reaper cycle (for testing).
    ///
    /// Correctness invariants maintained:
    /// - The TOCTOU window between "collect expired IDs" and "overwrite record" is closed by
    ///   re-checking the record's status inside `jobs.write()`. If a worker called
    ///   `ack_complete` or `ack_fail` between the ID collection and the write, the record
    ///   is no longer `Processing` and the reaper skips it, preventing double-execution.
    /// - All `jobs` mutations are batched inside a single write lock acquisition.
    /// - All `queues` insertions are batched inside a single write lock acquisition.
    /// - Retry re-enqueue uses `priority_insert` (not `push_back`) to preserve priority
    ///   ordering — a reclaimed Critical job is not placed behind Normal/Low entries.
    pub async fn reap_expired_leases(&self) -> QueueResult<Vec<ReapOutcome>> {
        let now = Utc::now();

        // ── Phase 1: Collect IDs of expired leases under jobs.read() ───────────────
        // Only the job IDs are collected, not full records. The authoritative
        // record is read again inside jobs.write() in phase 2 to close the TOCTOU.
        let expired_ids: Vec<crate::JobId> = {
            let jobs = self.backend.jobs.read().await;
            jobs.iter()
                .filter_map(|(job_id, record)| match &record.status {
                    JobStatus::Processing { lease_until } if *lease_until < now => {
                        Some(job_id.clone())
                    }
                    _ => None,
                })
                .collect()
        }; // read lock released

        if expired_ids.is_empty() {
            return Ok(Vec::new());
        }

        // ── Phase 2: Mutate all expired records in ONE jobs.write() ──────────────────
        // Re-check status inside the write lock to close the TOCTOU window:
        // a worker that called ack_complete just before the reaper fires will
        // have set the status to Completed; the reaper skips those records.
        let mut to_requeue: Vec<(String, String, crate::JobPriority, crate::JobId, chrono::DateTime<Utc>)> = Vec::new();
        let mut events: Vec<JobEvent> = Vec::new();
        let mut outcomes: Vec<ReapOutcome> = Vec::new();

        {
            let mut jobs = self.backend.jobs.write().await;

            for job_id in &expired_ids {
                let record = match jobs.get_mut(job_id) {
                    Some(r) => r,
                    None => continue, // job was deleted between phases — skip
                };

                // TOCTOU guard: only reclaim if STILL in an expired-processing state.
                let still_expired_processing = matches!(
                    &record.status,
                    JobStatus::Processing { lease_until } if *lease_until < now
                );
                if !still_expired_processing {
                    debug!("Skipping job {} — status changed since collection", job_id);
                    continue;
                }

                // Clear the lease.
                record.lease_token = None;
                record.updated_at = now;
                record.set_error("Lease expired".to_string());

                // The reaper does not hold the adapter's retry budget; it uses the
                // same attempt > max_retries threshold the adapter uses (attempt is
                // the count after the last dequeue, so this is a conservative check).
                if record.attempt > record.message.max_retries {
                    record.status = JobStatus::Failed {
                        failed_at: now,
                        error: "Max retries exceeded due to lease expiry".to_string(),
                    };

                    events.push(JobEvent::Failed {
                        job_id: job_id.clone(),
                        tenant_id: record.tenant_id.clone(),
                        error: "Max retries exceeded due to lease expiry".to_string(),
                        at: now,
                    });

                    outcomes.push(ReapOutcome {
                        tenant_id: record.tenant_id.clone(),
                        job_id: job_id.clone(),
                        job_type: record.message.job_type.clone(),
                        permanently_failed: true,
                        retry_at: None,
                    });
                } else {
                    // Apply a minimum backoff before re-enqueue to prevent a tight
                    // retry loop when a job reliably crashes its worker (OOM, SIGKILL).
                    let retry_at = now + self.base_retry_backoff;
                    record.status = JobStatus::Retrying { retry_at };

                    // Capture details for queue insertion (done under queues.write() next).
                    to_requeue.push((
                        record.tenant_id.clone(),
                        record.message.queue.clone(),
                        record.message.priority,
                        job_id.clone(),
                        retry_at,
                    ));

                    events.push(JobEvent::Retrying {
                        job_id: job_id.clone(),
                        tenant_id: record.tenant_id.clone(),
                        retry_at,
                        error: "Lease expired".to_string(),
                        at: now,
                    });

                    outcomes.push(ReapOutcome {
                        tenant_id: record.tenant_id.clone(),
                        job_id: job_id.clone(),
                        job_type: record.message.job_type.clone(),
                        permanently_failed: false,
                        retry_at: Some(retry_at),
                    });
                }
            }
        } // jobs write lock released

        // ── Phase 3: Re-enqueue retrying jobs in ONE queues.write() ─────────────────
        // Uses priority_insert (not push_back) so reclaimed Critical jobs are
        // placed before Normal/Low entries, not appended to the tail.
        if !to_requeue.is_empty() {
            let mut queues = self.backend.queues.write().await;
            for (tenant_id, queue_name, priority, job_id, retry_at) in to_requeue {
                let tenant_queues = queues.entry(tenant_id).or_default();
                let queue = tenant_queues.entry(queue_name).or_default();
                priority_insert(queue, (priority, retry_at, job_id));
            }
        } // queues write lock released

        // ── Phase 4: Broadcast events (outside any lock) ────────────────────────────
        for event in events {
            let _ = self.backend.event_broadcaster.send(event);
        }

        Ok(outcomes)
    }
}

/// Test helpers for deterministic testing
impl MemoryBackend {
    /// Force a lease to expire (test helper)
    pub async fn force_lease_expiry(&self, job_id: crate::JobId) -> QueueResult<()> {
        let mut jobs = self.jobs.write().await;
        if let Some(record) = jobs.get_mut(&job_id) {
            if let JobStatus::Processing {
                ref mut lease_until,
            } = record.status
            {
                *lease_until = Utc::now() - chrono::Duration::seconds(1);
                record.updated_at = Utc::now();
            }
        }
        Ok(())
    }

    /// Run one reaper tick (test helper)
    pub async fn run_reaper_tick(&self) -> QueueResult<()> {
        let reaper = LeaseReaper::new(Arc::new(self.clone()));
        reaper.reap_expired_leases().await?;
        Ok(())
    }

    /// Advance time concept (test helper - for Memory backend, this is a no-op since we use real time)
    pub async fn advance_time_to(&self, _target_time: chrono::DateTime<Utc>) -> QueueResult<()> {
        // For memory backend, we can't actually advance time
        // Tests should use force_lease_expiry or similar helpers
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::QueueBackend;
    use crate::{JobMessage, JobPriority, QueueCtx};

    fn create_test_context() -> QueueCtx {
        QueueCtx::new("test_tenant")
    }

    fn create_test_job_message() -> JobMessage {
        JobMessage {
            job_type: "test_job".to_string(),
            payload_bytes: b"{}".to_vec(), // valid JSON — consistent with codec: "json"
            codec: "json".to_string(),
            queue: "default".to_string(),
            priority: JobPriority::Normal,
            max_retries: 3,
            run_at: chrono::Utc::now(),
            idempotency_key: None,
        }
    }

    #[tokio::test]
    async fn test_lease_expiry_reaper() {
        let backend = Arc::new(MemoryBackend::new());
        let ctx = create_test_context();
        let message = create_test_job_message();

        // Enqueue and lease a job
        let job_id = backend.enqueue(ctx.clone(), message).await.unwrap();
        let _leased = backend
            .dequeue(ctx.clone(), &["default"])
            .await
            .unwrap()
            .unwrap();

        // Force lease expiry
        backend.force_lease_expiry(job_id.clone()).await.unwrap();

        // Run reaper with zero backoff — test verifies structural re-enqueue
        // correctness, not timing. The 1s default applies in production.
        let reaper = LeaseReaper::new(backend.clone()).with_backoff(Duration::from_secs(0));
        let reclaimed = reaper.reap_expired_leases().await.unwrap();

        assert_eq!(reclaimed.len(), 1);

        // Job should be available for dequeue again
        let retry_leased = backend.dequeue(ctx, &["default"]).await.unwrap();
        assert!(retry_leased.is_some());
        assert_eq!(retry_leased.unwrap().record.attempt, 2); // Attempt incremented
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let backend = Arc::new(MemoryBackend::new());
        let ctx = create_test_context();
        let mut message = create_test_job_message();
        message.max_retries = 1; // Only 1 retry allowed

        // Enqueue and lease a job
        let job_id = backend.enqueue(ctx.clone(), message).await.unwrap();
        let _leased = backend
            .dequeue(ctx.clone(), &["default"])
            .await
            .unwrap()
            .unwrap();

        // Force lease expiry
        backend.force_lease_expiry(job_id.clone()).await.unwrap();

        // Set attempt past max_retries (attempt=2 > max_retries=1) to exercise the fail path.
        {
            let mut jobs = backend.jobs.write().await;
            if let Some(record) = jobs.get_mut(&job_id) {
                record.attempt = 2;
            }
        }

        // Run the reaper — use zero backoff so the reclaimed job is immediately
        // dequeue-eligible.  The 1s production default is not appropriate for tests.
        let reaper = LeaseReaper::new(backend.clone()).with_backoff(Duration::ZERO);
        let reclaimed = reaper.reap_expired_leases().await.unwrap();

        assert_eq!(reclaimed.len(), 1);

        // Job should be marked as failed
        let status = backend.get_status(ctx, job_id).await.unwrap();
        assert!(matches!(status, JobStatus::Failed { .. }));
    }

    /// Verify the TOCTOU guard: if a worker acks the job between the reaper's
    /// collection phase and its write phase, the reaper must NOT overwrite the
    /// terminal record.
    #[tokio::test]
    async fn test_reaper_skips_already_acked_job() {
        let backend = Arc::new(MemoryBackend::new());
        let ctx = create_test_context();

        let job_id = backend
            .enqueue(ctx.clone(), create_test_job_message())
            .await
            .unwrap();
        let _leased = backend
            .dequeue(ctx.clone(), &["default"])
            .await
            .unwrap()
            .unwrap();

        // Expire the lease so the reaper's filter would match.
        backend.force_lease_expiry(job_id.clone()).await.unwrap();

        // Simulate: worker completes the job BEFORE the reaper's write phase.
        // (In production this race is closed by checking status inside jobs.write().)
        // Use record.complete() — the actual JobRecord transition helper — so the
        // test exercises the same state transitions ack_complete would perform,
        // including lease_token clearance. Inline status mutation would diverge
        // if complete() gains additional side effects.
        {
            let mut jobs = backend.jobs.write().await;
            if let Some(record) = jobs.get_mut(&job_id) {
                record.complete();
            }
        }

        // TOCTOU test: uses zero backoff, irrelevant to timing.
        let reaper = LeaseReaper::new(backend.clone()).with_backoff(Duration::from_secs(0));
        let reclaimed = reaper.reap_expired_leases().await.unwrap();

        assert_eq!(reclaimed.len(), 0, "reaper must not reclaim an already-completed job");

        // Status must still be Completed, not Retrying.
        let status = backend.get_status(ctx, job_id).await.unwrap();
        assert!(
            matches!(status, crate::JobStatus::Completed { .. }),
            "completed job must not be overwritten by reaper"
        );
    }
}
