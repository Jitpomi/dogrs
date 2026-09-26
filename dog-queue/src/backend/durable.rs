//! Shared state transitions for durable, tenant-scoped stores.
//! Stores must atomically read/modify/write one tenant's state across processes.
use super::{BoxStream, QueueBackend, ReapOutcome};
use crate::{
    types::LeaseToken, JobEvent, JobId, JobMessage, JobRecord, JobStatus, LeasedJob,
    QueueCapabilities, QueueCtx, QueueError, QueueResult,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct TenantState {
    jobs: HashMap<JobId, StoredRecord>,
}
#[derive(Serialize, Deserialize)]
struct StoredRecord {
    record: JobRecord,
    token: Option<LeaseToken>,
}

pub(crate) enum Operation {
    Enqueue(JobMessage),
    Dequeue(Vec<String>, Duration),
    Complete(JobId, LeaseToken, Option<String>),
    Fail(JobId, LeaseToken, String, Option<DateTime<Utc>>),
    Heartbeat(JobId, LeaseToken, Duration),
    Cancel(JobId),
    Get(JobId),
    Reap,
    Purge(DateTime<Utc>),
}
pub(crate) enum Outcome {
    Id(JobId),
    Lease(Option<LeasedJob>),
    Done,
    Canceled(bool),
    Record(JobRecord),
    Reaped(Vec<ReapOutcome>),
    Purged(usize),
}

impl TenantState {
    /// Reserve room for bounded terminal outcomes and lease metadata on every job.
    #[cfg(feature = "nats-async")]
    pub(crate) fn reserved_bytes(&self) -> usize {
        self.jobs.len().saturating_mul(8192)
    }

    pub(crate) fn apply(&mut self, tenant: &str, operation: &Operation) -> QueueResult<Outcome> {
        let now = Utc::now();
        match operation {
            Operation::Enqueue(message) => {
                if let Some(key) = &message.idempotency_key {
                    if let Some(existing) = self.jobs.values().find(|r| {
                        !r.record.status.is_terminal()
                            && r.record.message.queue == message.queue
                            && r.record.message.job_type == message.job_type
                            && r.record.message.idempotency_key.as_ref() == Some(key)
                    }) {
                        return Ok(Outcome::Id(existing.record.job_id.clone()));
                    }
                }
                let id = JobId::new();
                self.jobs.insert(
                    id.clone(),
                    StoredRecord {
                        record: JobRecord::new(id.clone(), tenant, message.clone()),
                        token: None,
                    },
                );
                Ok(Outcome::Id(id))
            }
            Operation::Dequeue(queues, duration) => {
                if duration.is_zero() {
                    return Err(QueueError::InvalidConfig(
                        "Lease duration must be positive".into(),
                    ));
                }
                let id = self
                    .jobs
                    .values()
                    .filter(|r| {
                        queues.contains(&r.record.message.queue)
                            && r.record.message.run_at <= now
                            && r.record.status.is_eligible(now)
                    })
                    .min_by_key(|r| {
                        (
                            std::cmp::Reverse(r.record.message.priority),
                            r.record.created_at,
                            r.record.job_id.clone(),
                        )
                    })
                    .map(|r| r.record.job_id.clone());
                let Some(id) = id else {
                    return Ok(Outcome::Lease(None));
                };
                let until = now
                    .checked_add_signed(
                        chrono::Duration::from_std(*duration)
                            .map_err(|e| QueueError::InvalidConfig(e.to_string()))?,
                    )
                    .ok_or_else(|| QueueError::InvalidConfig("Lease duration overflow".into()))?;
                let row = self.jobs.get_mut(&id).unwrap();
                let token = LeaseToken::new();
                row.record.attempt += 1;
                row.record.start_processing(token.clone(), until);
                row.token = Some(token.clone());
                Ok(Outcome::Lease(Some(LeasedJob::new(
                    row.record.clone(),
                    token,
                    until,
                ))))
            }
            Operation::Get(id) => Ok(Outcome::Record(
                self.jobs
                    .get(id)
                    .ok_or_else(|| QueueError::JobNotFound(id.clone()))?
                    .record
                    .clone(),
            )),
            Operation::Cancel(id) => {
                let Some(row) = self.jobs.get_mut(id) else {
                    return Ok(Outcome::Canceled(false));
                };
                if row.record.status.is_terminal() {
                    return Ok(Outcome::Canceled(false));
                }
                row.record.cancel();
                row.token = None;
                Ok(Outcome::Canceled(true))
            }
            Operation::Purge(before) => {
                let old = self.jobs.len();
                self.jobs.retain(|_, r| {
                    !r.record.status.is_terminal() || r.record.updated_at >= *before
                });
                Ok(Outcome::Purged(old - self.jobs.len()))
            }
            Operation::Reap => {
                let mut outcomes = Vec::new();
                for row in self
                    .jobs
                    .values_mut()
                    .filter(|r| r.record.lease_expired(now))
                {
                    let exhausted = row.record.attempt > row.record.message.max_retries;
                    let retry_at = if exhausted {
                        None
                    } else {
                        Some(now + chrono::Duration::seconds(1))
                    };
                    row.record.set_error("Lease expired".into());
                    if let Some(at) = retry_at {
                        row.record.schedule_retry(at);
                    } else {
                        row.record.fail("Lease expired".into());
                    }
                    row.token = None;
                    outcomes.push(ReapOutcome {
                        tenant_id: tenant.into(),
                        job_id: row.record.job_id.clone(),
                        job_type: row.record.message.job_type.clone(),
                        permanently_failed: exhausted,
                        retry_at,
                    });
                }
                Ok(Outcome::Reaped(outcomes))
            }
            Operation::Complete(id, token, _)
            | Operation::Fail(id, token, _, _)
            | Operation::Heartbeat(id, token, _) => {
                let row = self
                    .jobs
                    .get_mut(id)
                    .ok_or_else(|| QueueError::JobNotFound(id.clone()))?;
                if matches!(row.record.status, JobStatus::Canceled { .. }) {
                    return Err(QueueError::JobCanceled);
                }
                if row.record.status.is_terminal() {
                    return Err(QueueError::JobAlreadyTerminal);
                }
                if row.token.as_ref() != Some(token) || !row.record.status.is_processing() {
                    return Err(QueueError::InvalidLeaseToken { job_id: id.clone() });
                }
                if row.record.lease_expired(now) {
                    return Err(QueueError::LeaseExpired);
                }
                match operation {
                    Operation::Complete(_, _, result) => {
                        if serde_json::to_vec(result)
                            .map_err(|e| QueueError::SerializationError(e.to_string()))?
                            .len()
                            > 4096
                        {
                            return Err(QueueError::InvalidConfig("Persisted result must fit in 4 KiB; store large results by reference".into()));
                        }
                        row.record.complete();
                        row.record.result = result.clone();
                        row.token = None;
                    }
                    Operation::Fail(_, _, error, retry) => {
                        let error: String = error.chars().take(256).collect();
                        row.record.set_error(error.clone());
                        match retry {
                            Some(at) if row.record.attempt <= row.record.message.max_retries => {
                                row.record.schedule_retry(*at)
                            }
                            _ => row.record.fail(error),
                        }
                        row.token = None;
                    }
                    Operation::Heartbeat(_, _, duration) => {
                        let until = row
                            .record
                            .lease_until()
                            .unwrap()
                            .checked_add_signed(
                                chrono::Duration::from_std(*duration)
                                    .map_err(|e| QueueError::InvalidConfig(e.to_string()))?,
                            )
                            .ok_or_else(|| {
                                QueueError::InvalidConfig("Lease duration overflow".into())
                            })?;
                        row.record.start_processing(token.clone(), until);
                    }
                    _ => unreachable!(),
                }
                Ok(Outcome::Done)
            }
        }
    }
}

#[async_trait]
pub(crate) trait StateStore: Send + Sync {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome>;
    async fn tenants(&self) -> QueueResult<Vec<String>>;
}

pub struct DurableBackend<S> {
    pub(crate) store: S,
    pub(crate) lease_duration: Duration,
}
impl<S> DurableBackend<S> {
    pub fn with_lease_duration(mut self, duration: Duration) -> Self {
        self.lease_duration = duration;
        self
    }
}

#[async_trait]
impl<S: StateStore> QueueBackend for DurableBackend<S> {
    async fn enqueue(&self, ctx: QueueCtx, msg: JobMessage) -> QueueResult<JobId> {
        match self
            .store
            .update(&ctx.tenant_id, &Operation::Enqueue(msg))
            .await?
        {
            Outcome::Id(id) => Ok(id),
            _ => unreachable!(),
        }
    }
    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
        match self
            .store
            .update(
                &ctx.tenant_id,
                &Operation::Dequeue(
                    queues.iter().map(|s| s.to_string()).collect(),
                    self.lease_duration,
                ),
            )
            .await?
        {
            Outcome::Lease(job) => Ok(job),
            _ => unreachable!(),
        }
    }
    async fn ack_complete(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        result: Option<String>,
    ) -> QueueResult<()> {
        self.store
            .update(&ctx.tenant_id, &Operation::Complete(id, token, result))
            .await?;
        Ok(())
    }
    async fn ack_fail(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        error: String,
        retry: Option<DateTime<Utc>>,
    ) -> QueueResult<()> {
        self.store
            .update(&ctx.tenant_id, &Operation::Fail(id, token, error, retry))
            .await?;
        Ok(())
    }
    async fn heartbeat_extend(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        duration: Duration,
    ) -> QueueResult<()> {
        self.store
            .update(&ctx.tenant_id, &Operation::Heartbeat(id, token, duration))
            .await?;
        Ok(())
    }
    async fn cancel(&self, ctx: QueueCtx, id: JobId) -> QueueResult<bool> {
        match self
            .store
            .update(&ctx.tenant_id, &Operation::Cancel(id))
            .await?
        {
            Outcome::Canceled(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    async fn get_status(&self, ctx: QueueCtx, id: JobId) -> QueueResult<JobStatus> {
        Ok(self.get_record(ctx, id).await?.status)
    }
    async fn get_record(&self, ctx: QueueCtx, id: JobId) -> QueueResult<JobRecord> {
        match self
            .store
            .update(&ctx.tenant_id, &Operation::Get(id))
            .await?
        {
            Outcome::Record(record) => Ok(record),
            _ => unreachable!(),
        }
    }
    fn event_stream(&self, _: QueueCtx) -> BoxStream<JobEvent> {
        Box::pin(futures::stream::empty())
    }
    async fn reclaim_expired_leases(&self) -> QueueResult<Vec<ReapOutcome>> {
        let mut results = Vec::new();
        for tenant in self.store.tenants().await? {
            if let Outcome::Reaped(mut rows) = self.store.update(&tenant, &Operation::Reap).await? {
                results.append(&mut rows);
            }
        }
        Ok(results)
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

#[allow(private_bounds)]
impl<S: StateStore> DurableBackend<S> {
    /// Remove terminal history before a caller-chosen cutoff, without touching active jobs.
    pub async fn purge_terminal_before(
        &self,
        ctx: QueueCtx,
        before: DateTime<Utc>,
    ) -> QueueResult<usize> {
        match self
            .store
            .update(&ctx.tenant_id, &Operation::Purge(before))
            .await?
        {
            Outcome::Purged(count) => Ok(count),
            _ => unreachable!(),
        }
    }
}

impl<S: StateStore> super::broker::sealed::Sealed for DurableBackend<S> {}
impl<S: StateStore> super::broker::JobLedger for DurableBackend<S> {}
