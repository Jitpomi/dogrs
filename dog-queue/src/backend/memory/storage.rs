use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
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
}

impl MemoryBackend {
    pub fn new() -> Self {
        let (event_broadcaster, _) = broadcast::channel(1000);

        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            queues: Arc::new(RwLock::new(HashMap::new())),
            idempotency: Arc::new(RwLock::new(HashMap::new())),
            event_broadcaster,
        }
    }
}

#[async_trait]
impl QueueBackend for MemoryBackend {
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
        // Check idempotency if key provided
        if let Some(ref key) = message.idempotency_key {
            let idempotency_scope = (
                ctx.tenant_id.clone(),
                message.queue.clone(),
                message.job_type.clone(),
                key.clone(),
            );

            let idempotency = self.idempotency.read();
            if let Some(existing_job_id) = idempotency.get(&idempotency_scope) {
                // Check if existing job is terminal
                let jobs = self.jobs.read();
                if let Some(existing_record) = jobs.get(existing_job_id) {
                    match existing_record.status {
                        JobStatus::Completed { .. }
                        | JobStatus::Failed { .. }
                        | JobStatus::Canceled { .. } => {
                            // Terminal job - allow new enqueue
                        }
                        _ => {
                            // Non-terminal - return existing job_id
                            return Ok(existing_job_id.clone());
                        }
                    }
                }
            }
        }

        let job_id = JobId::new();
        let now = Utc::now();

        // Create job record
        let record = JobRecord::new(job_id.clone(), ctx.tenant_id.clone(), message.clone());

        // Store job record
        self.jobs.write().insert(job_id.clone(), record);

        // Add to queue
        let mut queues = self.queues.write();
        let tenant_queues = queues.entry(ctx.tenant_id.clone()).or_default();
        let queue = tenant_queues.entry(message.queue.clone()).or_default();

        // Insert in priority order (higher priority first, then FIFO within same priority).
        // Priority is stored in the queue entry — no cross-lock read of `jobs` needed.
        let insert_pos = queue
            .iter()
            .position(|(existing_priority, _, _)| message.priority > *existing_priority)
            .unwrap_or(queue.len());
        queue.insert(insert_pos, (message.priority, message.run_at, job_id.clone()));
        drop(queues);

        // Update idempotency tracking
        if let Some(ref key) = message.idempotency_key {
            let idempotency_scope = (
                ctx.tenant_id.clone(),
                message.queue.clone(),
                message.job_type.clone(),
                key.clone(),
            );
            self.idempotency
                .write()
                .insert(idempotency_scope, job_id.clone());
        }

        // Emit event
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
            let candidate = {
                let mut queues_lock = self.queues.write();
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
                let mut jobs = self.jobs.write();
                if let Some(record) = jobs.get_mut(&job_id) {
                    match &record.status {
                        JobStatus::Enqueued | JobStatus::Retrying { .. } => {
                            let lease_token = LeaseToken::new();
                            let lease_until = now + chrono::Duration::seconds(300); // 5 minute lease

                            record.attempt += 1;
                            record.start_processing(lease_token.clone(), lease_until);

                            let event = JobEvent::Leased {
                                job_id: job_id.clone(),
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
        _result_ref: Option<String>,
    ) -> QueueResult<()> {
        let now = Utc::now();
        let mut jobs = self.jobs.write();

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.to_string()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }

        // Check for cancellation (cancel-wins)
        if matches!(record.status, JobStatus::Canceled { .. }) {
            return Err(QueueError::JobCanceled);
        }

        // Check for other terminal states
        match &record.status {
            JobStatus::Completed { .. } | JobStatus::Failed { .. } => {
                return Err(QueueError::JobAlreadyTerminal);
            }
            _ => {}
        }

        // Verify lease token
        if record.lease_token.as_ref() != Some(&lease_token) {
            return Err(QueueError::InvalidLeaseToken);
        }

        // Check lease expiry
        if let Some(lease_until) = record.lease_until {
            if now > lease_until {
                return Err(QueueError::LeaseExpired);
            }
        }

        // Update to completed
        record.complete();

        // Emit event
        let event = JobEvent::Completed {
            job_id: job_id.clone(),
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
        let mut jobs = self.jobs.write();

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.to_string()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.to_string()));
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
            return Err(QueueError::InvalidLeaseToken);
        }

        // Check lease expiry
        if let Some(lease_until) = record.lease_until {
            if now > lease_until {
                return Err(QueueError::LeaseExpired);
            }
        }

        // Check if max retries exceeded
        if record.attempt >= record.message.max_retries {
            record.fail(format!("Max retries exceeded: {}", error));

            let event = JobEvent::Failed {
                job_id: job_id.clone(),
                error: format!("Max retries exceeded: {}", error),
                at: now,
            };
            let _ = self.event_broadcaster.send(event);
        } else if let Some(retry_time) = retry_at {
            // Schedule retry
            record.schedule_retry(retry_time);
            record.set_error(error.clone());

            // Re-add to queue for retry, preserving priority ordering.
            let mut queues = self.queues.write();
            let priority = record.message.priority;
            let tenant_queues = queues.entry(ctx.tenant_id.clone()).or_default();
            let queue = tenant_queues
                .entry(record.message.queue.clone())
                .or_default();
            queue.push_back((priority, retry_time, job_id.clone()));

            let event = JobEvent::Retrying {
                job_id: job_id.clone(),
                retry_at: retry_time,
                error: error.clone(),
                at: now,
            };
            let _ = self.event_broadcaster.send(event);
        } else {
            // Permanent failure
            record.fail(error.clone());

            let event = JobEvent::Failed {
                job_id: job_id.clone(),
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
        let mut jobs = self.jobs.write();

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.to_string()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }

        // Check for cancellation (cancel-wins)
        if matches!(record.status, JobStatus::Canceled { .. }) {
            return Err(QueueError::JobCanceled);
        }

        // Verify lease token
        if record.lease_token.as_ref() != Some(&lease_token) {
            return Err(QueueError::InvalidLeaseToken);
        }

        // Extend lease
        if let Some(ref mut lease_until) = record.lease_until {
            *lease_until += chrono::Duration::from_std(extra_time).unwrap();
            record.updated_at = now;
        }

        Ok(())
    }

    async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool> {
        let now = Utc::now();
        let mut jobs = self.jobs.write();

        let record = jobs
            .get_mut(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.to_string()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }

        // Check if already terminal
        match &record.status {
            JobStatus::Completed { .. } | JobStatus::Failed { .. } | JobStatus::Canceled { .. } => {
                return Ok(false); // Already terminal
            }
            _ => {}
        }

        // Cancel the job
        record.status = JobStatus::Canceled { canceled_at: now };
        record.lease_token = None; // Invalidate lease
        record.lease_until = None;
        record.updated_at = now;

        // Emit event
        let event = JobEvent::Canceled {
            job_id: job_id.clone(),
            at: now,
        };
        let _ = self.event_broadcaster.send(event);

        Ok(true)
    }

    async fn get_status(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus> {
        let jobs = self.jobs.read();
        let record = jobs
            .get(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.to_string()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }

        Ok(record.status.clone())
    }

    async fn get_record(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobRecord> {
        let jobs = self.jobs.read();
        let record = jobs
            .get(&job_id)
            .ok_or_else(|| QueueError::JobNotFound(job_id.to_string()))?;

        // Verify tenant access
        if record.tenant_id != ctx.tenant_id {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }

        Ok(record.clone())
    }

    fn event_stream(&self, _ctx: QueueCtx) -> BoxStream<JobEvent> {
        let receiver = self.event_broadcaster.subscribe();
        use tokio_stream::{wrappers::BroadcastStream, StreamExt};
        let stream = BroadcastStream::new(receiver).filter_map(|result| result.ok());

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
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobMessage, JobPriority};

    fn create_test_context() -> QueueCtx {
        QueueCtx::new("test_tenant".to_string())
    }

    fn create_test_job_message() -> JobMessage {
        JobMessage {
            job_type: "test_job".to_string(),
            payload_bytes: b"test_payload".to_vec(),
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
