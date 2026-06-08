use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{
    backend::{BoxStream, QueueBackend},
    types::LeaseToken,
    JobEvent, JobId, JobMessage, JobRecord, JobStatus, LeasedJob, QueueCapabilities, QueueCtx,
    QueueError, QueueResult,
};

// Type aliases to reduce complexity.
// Each queue entry stores (priority, run_at, job_id) so that:
//   - `enqueue` can compare priorities without locking `jobs`
//   - `dequeue` can check eligibility (run_at <= now) without locking `jobs`
// This eliminates all nested-lock cross-reads between `queues` and `jobs`.
type QueueEntry = (crate::JobPriority, DateTime<Utc>, JobId);
type TenantQueues = HashMap<String, HashMap<String, VecDeque<QueueEntry>>>;
type IdempotencyMap = HashMap<(String, String, String, String), JobId>;

// ---------------------------------------------------------------------------
// Priority-ordered insertion helper
// ---------------------------------------------------------------------------

/// Insert `entry` into a priority-ordered deque.
///
/// Entries are ordered descending by priority (Critical first, Low last).
/// Within the same priority, insertion order is preserved (FIFO).
///
/// `position(|p| new_priority > *p)` finds the first slot whose incumbent
/// has a strictly lower priority — the new entry is inserted before it,
/// preserving FIFO order for entries of equal priority.
///
/// **Performance note**: this is O(n) scan + O(n) shift — acceptable for the
/// in-memory development backend. A production backend should use a
/// `BinaryHeap` or per-priority `VecDeque` tiers for O(log n) / O(1) ops.
pub(super) fn priority_insert(queue: &mut VecDeque<QueueEntry>, entry: QueueEntry) {
    let pos = queue
        .iter()
        .position(|(p, _, _)| entry.0 > *p)
        .unwrap_or(queue.len());
    queue.insert(pos, entry);
}

/// In-memory backend for testing and development
pub struct MemoryBackend {
    /// Job records indexed by job_id
    pub(crate) jobs: Arc<RwLock<HashMap<JobId, JobRecord>>>,

    /// Queue storage: tenant_id -> queue_name -> job_ids (priority ordered)
    pub(crate) queues: Arc<RwLock<TenantQueues>>,

    /// Idempotency tracking: (tenant_id, queue, job_type, key) -> job_id
    pub(crate) idempotency: Arc<RwLock<IdempotencyMap>>,

    /// Event broadcaster for observability
    pub(crate) event_broadcaster: broadcast::Sender<JobEvent>,

    /// How long a dequeued lease is valid. Defaults to 5 minutes.
    /// Set via `MemoryBackend::with_lease_duration`.
    pub(crate) lease_duration: chrono::Duration,
}

impl MemoryBackend {
    pub fn new() -> Self {
        let (event_broadcaster, _) = broadcast::channel(1000);

        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            queues: Arc::new(RwLock::new(HashMap::new())),
            idempotency: Arc::new(RwLock::new(HashMap::new())),
            event_broadcaster,
            lease_duration: chrono::Duration::seconds(300), // 5-minute default
        }
    }

    /// Override the default 5-minute lease duration.
    /// Use a shorter value (e.g. 30 s) in tests to exercise the reaper.
    pub fn with_lease_duration(mut self, duration: std::time::Duration) -> Self {
        self.lease_duration = chrono::Duration::from_std(duration)
            .expect("lease_duration is out of chrono::Duration range");
        self
    }
}

#[async_trait]
impl QueueBackend for MemoryBackend {
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
        // Compute the idempotency scope once (avoids repeated clones below).
        let idempotency_scope: Option<(String, String, String, String)> =
            message.idempotency_key.as_ref().map(|key| {
                (
                    ctx.tenant_id.clone(),
                    message.queue.clone(),
                    message.job_type.clone(),
                    key.clone(),
                )
            });

        // Acquire the idempotency write lock *before* the existence check and hold
        // it until the new entry is committed.  This closes the TOCTOU window:
        // two concurrent enqueues with the same key both need this lock, so only
        // one proceeds past the check at a time.
        //
        // Lock ordering (always observed): idempotency → jobs → queues.
        // No other method in this backend acquires idempotency, so no deadlock risk.
        let mut idempotency_guard = self.idempotency.write().await;

        if let Some(ref scope) = idempotency_scope {
            if let Some(existing_id) = idempotency_guard.get(scope).cloned() {
                // Check terminal status under jobs.read().
                // Holding idempotency.write() while acquiring jobs.read() is safe
                // because no other code path holds jobs.write() and then tries to
                // acquire idempotency (only enqueue does, and it's now serialised).
                let jobs = self.jobs.read().await;
                if let Some(record) = jobs.get(&existing_id) {
                    if !record.status.is_terminal() {
                        // Non-terminal — deduplicate and return the existing id.
                        return Ok(existing_id);
                    }
                    // Terminal — fall through and create a new job below.
                }
                // Existing id not found in jobs (possible after a GC pass) —
                // fall through and re-enqueue with a fresh id.
            }
        }

        let job_id = JobId::new();
        let now = Utc::now();

        // Create and store the job record.
        let record = JobRecord::new(job_id.clone(), &ctx.tenant_id, message.clone());
        self.jobs.write().await.insert(job_id.clone(), record);

        // Insert into the priority-ordered queue.
        let mut queues = self.queues.write().await;
        let tenant_queues = queues.entry(ctx.tenant_id.clone()).or_default();
        let queue = tenant_queues.entry(message.queue.clone()).or_default();
        priority_insert(queue, (message.priority, message.run_at, job_id.clone()));
        drop(queues);

        // Register/update the idempotency entry (still under the write lock — no race).
        if let Some(scope) = idempotency_scope {
            idempotency_guard.insert(scope, job_id.clone());
        }
        drop(idempotency_guard);

        // Emit enqueue event after all locks are released.
        let event = JobEvent::Enqueued {
            job_id: job_id.clone(),
            tenant_id: ctx.tenant_id.clone(),
            queue: message.queue.clone(),
            job_type: message.job_type.clone(),
            at: now,
        };
        let _ = self.event_broadcaster.send(event);

        Ok(job_id)
    }

    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
        let now = Utc::now();

        for queue_name in queues {
            // ── Phase 1: scan queue for eligible entry ────────────────────────────────
            // Hold queues.write() only. `run_at` is stored in the entry, so
            // eligibility is checked here without touching the `jobs` map.
            // Canceled entries whose run_at has passed are removed here and
            // discarded in phase 2 (lazy tombstone cleanup).
            //
            // Performance note: `position()` is O(n) over the VecDeque.
            // Entries are ordered (priority DESC, run_at ASC), so future-dated
            // high-priority entries at the head are scanned every poll.
            // For in-memory use at small scale this is acceptable; a split
            // ready-queue / future-heap structure would make this O(1).
            let candidate = {
                let mut queues_lock = self.queues.write().await;
                queues_lock
                    .get_mut(&ctx.tenant_id)
                    .and_then(|tq| tq.get_mut(*queue_name))
                    .and_then(|queue| {
                        let pos = queue
                            .iter()
                            .position(|(_, run_at, _)| *run_at <= now);
                        pos.map(|i| {
                            let (_, _, job_id) = queue.remove(i).unwrap();
                            job_id
                        })
                    })
            }; // queues_lock RELEASED

            // ── Phase 2: lease the job — single jobs.write() ──────────────────────────
            if let Some(job_id) = candidate {
                let mut jobs = self.jobs.write().await;
                if let Some(record) = jobs.get_mut(&job_id) {
                    match &record.status {
                        JobStatus::Enqueued | JobStatus::Retrying { .. } => {
                            let lease_token = LeaseToken::new();
                            let lease_until = now + self.lease_duration;

                            record.attempt += 1;
                            record.start_processing(lease_token.clone(), lease_until);

                            let event = JobEvent::Leased {
                                job_id: job_id.clone(),
                                tenant_id: record.tenant_id.clone(),
                                lease_until,
                                at: now,
                            };
                            let _ = self.event_broadcaster.send(event);

                            return Ok(Some(LeasedJob {
                                record: record.clone(),
                                lease_token,
                                lease_until,
                            }));
                        }
                        _ => {
                            // Job was canceled while queued — entry already removed
                            // from the queue in phase 1 (lazy tombstone). Skip to
                            // the next queue name.
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn ack_complete(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
        lease_token: LeaseToken,
        result_ref: Option<String>,
    ) -> QueueResult<()> {
        let now = Utc::now();
        let mut jobs = self.jobs.write().await;

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.clone()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.clone()));
        }

        // Unified terminal-state guard — mirrors ack_fail so a new terminal
        // variant only needs to be added in one place for both methods.
        match &record.status {
            JobStatus::Canceled { .. } => return Err(QueueError::JobCanceled),
            JobStatus::Completed { .. } | JobStatus::Failed { .. } => {
                return Err(QueueError::JobAlreadyTerminal);
            }
            _ => {}
        }

        // Verify lease token
        if record.lease_token.as_ref() != Some(&lease_token) {
            return Err(QueueError::InvalidLeaseToken { job_id: job_id.clone() });
        }

        // Check lease expiry — read from the status enum (single source of truth).
        if let Some(lease_until) = record.lease_until() {
            if now > lease_until {
                return Err(QueueError::LeaseExpired);
            }
        }

        // Store the serialized result so callers can retrieve it via get_result().
        record.result = result_ref;

        // Update to completed
        record.complete();

        // Emit event
        let event = JobEvent::Completed {
            job_id: job_id.clone(),
            tenant_id: ctx.tenant_id.clone(),
            at: now,
        };
        let _ = self.event_broadcaster.send(event);

        Ok(())
    }

    async fn ack_fail(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
        lease_token: LeaseToken,
        error: String,
        retry_at: Option<DateTime<Utc>>,
    ) -> QueueResult<()> {
        let now = Utc::now();
        let mut jobs = self.jobs.write().await;

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.clone()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.clone()));
        }

        // Cancel-wins: checked before the generic terminal-state guard so that
        // ack_fail returns JobCanceled (not JobAlreadyTerminal) for canceled jobs,
        // consistent with ack_complete.
        match &record.status {
            JobStatus::Canceled { .. } => return Err(QueueError::JobCanceled),
            JobStatus::Completed { .. } | JobStatus::Failed { .. } => {
                return Err(QueueError::JobAlreadyTerminal)
            }
            _ => {}
        }

        // Verify lease token
        if record.lease_token.as_ref() != Some(&lease_token) {
            return Err(QueueError::InvalidLeaseToken { job_id: job_id.clone() });
        }

        // Check lease expiry — read from the status enum (single source of truth).
        if let Some(lease_until) = record.lease_until() {
            if now > lease_until {
                return Err(QueueError::LeaseExpired);
            }
        }

        // The adapter is the sole authority for retry decisions: it computes
        // retry_at = Some(time) for retryable failures within budget, and None
        // for permanent failures or exhausted retries.  The backend trusts this
        // decision completely — do NOT re-check attempt counts here, which would
        // create a second source of truth and corrupt error messages.
        if let Some(retry_time) = retry_at {
            // Schedule retry: re-insert into the priority-ordered queue.
            // Use priority_insert (not push_back) so the retrying job is placed
            // at the correct position — push_back would cause priority inversion,
            // processing Critical retries after newly-enqueued Low jobs.
            record.schedule_retry(retry_time);
            record.set_error(error.clone());

            let mut queues = self.queues.write().await;
            let priority = record.message.priority;
            let queue_name = record.message.queue.clone();
            let tenant_queues = queues.entry(ctx.tenant_id.clone()).or_default();
            let queue = tenant_queues.entry(queue_name).or_default();
            priority_insert(queue, (priority, retry_time, job_id.clone()));

            let event = JobEvent::Retrying {
                job_id: job_id.clone(),
                tenant_id: ctx.tenant_id.clone(),
                retry_at: retry_time,
                error: error.clone(),
                at: now,
            };
            let _ = self.event_broadcaster.send(event);
        } else {
            // Permanent failure: record as-is with the verbatim error.
            record.fail(error.clone());

            let event = JobEvent::Failed {
                job_id: job_id.clone(),
                tenant_id: ctx.tenant_id.clone(),
                error,
                at: now,
            };
            let _ = self.event_broadcaster.send(event);
        }

        Ok(())
    }

    async fn heartbeat_extend(
        &self,
        ctx: QueueCtx,
        job_id: JobId,
        lease_token: LeaseToken,
        extra_time: std::time::Duration,
    ) -> QueueResult<()> {
        let now = Utc::now();
        let mut jobs = self.jobs.write().await;

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.clone()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.clone()));
        }

        // Check for cancellation (cancel-wins)
        if matches!(record.status, JobStatus::Canceled { .. }) {
            return Err(QueueError::JobCanceled);
        }

        // Verify lease token
        if record.lease_token.as_ref() != Some(&lease_token) {
            return Err(QueueError::InvalidLeaseToken { job_id: job_id.clone() });
        }

        // Explicitly guard that the job is still Processing before extending.
        // While functionally safe today (lease_until is Some only while Processing),
        // an implicit invariant across four methods is fragile under refactoring.
        if !matches!(record.status, JobStatus::Processing { .. }) {
            return Err(QueueError::Internal(format!(
                "heartbeat_extend called on job {} in '{}' state (must be Processing)",
                job_id,
                record.status.name(),
            )));
        }

        // Update the lease deadline inside JobStatus::Processing — the single
        // authoritative source. Updating only a separate field while leaving
        // the status enum stale caused the reaper to prematurely reclaim
        // heartbeat-extended jobs (the reaper reads from the status enum).
        if let JobStatus::Processing { ref mut lease_until } = record.status {
            let extra = chrono::Duration::from_std(extra_time).map_err(|e| {
                QueueError::Internal(format!("Invalid heartbeat duration: {e}"))
            })?;
            *lease_until += extra;
            record.updated_at = now;
        }

        // Capture the new deadline before releasing the write lock.
        let new_lease_until = record.lease_until().unwrap_or(now);
        drop(jobs);

        // Emit event outside the lock so subscribers don't block mutations.
        let event = JobEvent::HeartbeatExtended {
            job_id: job_id.clone(),
            tenant_id: ctx.tenant_id.clone(),
            new_lease_until,
            at: now,
        };
        let _ = self.event_broadcaster.send(event);

        Ok(())
    }

    async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
        let now = Utc::now();
        let mut jobs = self.jobs.write().await;

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.clone()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.clone()));
        }

        // Check if already terminal
        match &record.status {
            JobStatus::Completed { .. } | JobStatus::Failed { .. } | JobStatus::Canceled { .. } => {
                return Ok(false); // Already terminal
            }
            _ => {}
        }

        // Delegate to the JobRecord transition helper — consistent with complete(),
        // fail(), and schedule_retry() used by the other ack methods. This ensures
        // all cancellation-side effects (status, lease_token, updated_at) stay in
        // sync with any future additions to JobRecord::cancel().
        record.cancel();

        // Emit event
        let event = JobEvent::Canceled {
            job_id: job_id.clone(),
            tenant_id: ctx.tenant_id.clone(),
            at: now,
        };
        let _ = self.event_broadcaster.send(event);

        Ok(true)
    }

    async fn get_status(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus> {
        let jobs = self.jobs.read().await;
        let record = jobs
            .get(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.clone()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.clone()));
        }

        Ok(record.status.clone())
    }

    async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
        let jobs = self.jobs.read().await;
        let record = jobs
            .get(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.clone()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.clone()));
        }

        Ok(record.clone())
    }

    fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent> {
        let receiver = self.event_broadcaster.subscribe();
        use tokio_stream::{wrappers::BroadcastStream, StreamExt};
        let tenant_id = ctx.tenant_id;
        // Filter events so each tenant only receives events from their own jobs.
        // JobEvent::tenant_id() returns the originating tenant for every variant.
        let stream = BroadcastStream::new(receiver)
            .filter_map(|result| result.ok())
            .filter(move |e| e.tenant_id() == tenant_id);
        Box::pin(stream)
    }

    fn capabilities(&self) -> QueueCapabilities {
        QueueCapabilities {
            delayed: true,
            scheduled_at: true,
            cancel: true,
            lease_extend: true,
            priority: true,
            idempotency: true,
            dead_letter_queue: false,
        }
    }

    /// Reclaim expired leases using the built-in `LeaseReaper`.
    ///
    /// `MemoryBackend::clone()` clones the `Arc<RwLock<>>` fields (not the underlying
    /// maps), so the temporary reaper operates on the same shared data as this instance.
    async fn reclaim_expired_leases(&self) -> QueueResult<Vec<crate::backend::ReapOutcome>> {
        let reaper = crate::backend::memory::reaper::LeaseReaper::new(
            std::sync::Arc::new(self.clone()),
        );
        reaper.reap_expired_leases().await
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MemoryBackend {
    /// Clone the `MemoryBackend` handle.
    ///
    /// All `Arc<RwLock<…>>` fields are cloned — both the original and the clone
    /// share the **same** underlying data (jobs, queues, idempotency map).
    /// `broadcast::Sender::clone()` likewise shares the same broadcast channel.
    ///
    /// This is used by `LeaseReaper` and test helpers that need a separate handle
    /// to the same shared backend state.
    fn clone(&self) -> Self {
        Self {
            jobs: self.jobs.clone(),
            queues: self.queues.clone(),
            idempotency: self.idempotency.clone(),
            event_broadcaster: self.event_broadcaster.clone(),
            lease_duration: self.lease_duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobMessage, JobPriority};

    fn create_test_context() -> QueueCtx {
        QueueCtx::new("test_tenant")
    }

    fn create_test_job_message() -> JobMessage {
        JobMessage {
            job_type: "test_job".to_string(),
            payload_bytes: b"{}" .to_vec(),
            codec: "json".to_string(),
            queue: "default".to_string(),
            priority: JobPriority::Normal,
            max_retries: 3,
            run_at: chrono::Utc::now(),
            idempotency_key: None,
        }
    }

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let backend = MemoryBackend::new();
        let ctx = create_test_context();
        let message = create_test_job_message();

        // Enqueue
        let job_id = backend.enqueue(ctx.clone(), message).await.unwrap();

        // Dequeue
        let leased = backend.dequeue(ctx, &["default"]).await.unwrap().unwrap();
        assert_eq!(leased.record.job_id, job_id);
        assert_eq!(leased.record.attempt, 1);
    }

    #[tokio::test]
    async fn test_idempotency() {
        let backend = MemoryBackend::new();
        let ctx = create_test_context();
        let mut message = create_test_job_message();
        message.idempotency_key = Some("test_key".to_string());

        // First enqueue
        let job_id1 = backend.enqueue(ctx.clone(), message.clone()).await.unwrap();

        // Second enqueue with same key
        let job_id2 = backend.enqueue(ctx, message).await.unwrap();

        // Should return same job ID
        assert_eq!(job_id1, job_id2);
    }

    #[tokio::test]
    async fn test_cancel_wins() {
        let backend = MemoryBackend::new();
        let ctx = create_test_context();
        let message = create_test_job_message();

        let job_id = backend.enqueue(ctx.clone(), message).await.unwrap();
        let leased = backend
            .dequeue(ctx.clone(), &["default"])
            .await
            .unwrap()
            .unwrap();

        // Cancel job
        let canceled = backend.cancel(ctx.clone(), job_id.clone()).await.unwrap();
        assert!(canceled);

        // Try to ack_complete
        let result = backend
            .ack_complete(ctx, job_id, leased.lease_token, None)
            .await;
        assert!(matches!(result, Err(QueueError::JobCanceled)));
    }
}
