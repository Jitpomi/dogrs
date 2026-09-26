//! Shared transitions for a durable, tenant-scoped selection of job records.
//! Stores must atomically lock or compare-and-swap the records they select.
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
    pub(crate) jobs: HashMap<JobId, StoredRecord>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredRecord {
    pub(crate) record: JobRecord,
    pub(crate) token: Option<LeaseToken>,
}

pub(crate) enum Operation {
    Enqueue(JobMessage),
    Dequeue(Vec<String>, Duration),
    Complete(JobId, LeaseToken, Option<String>),
    Fail(JobId, LeaseToken, String, Option<DateTime<Utc>>),
    Heartbeat(JobId, LeaseToken, Duration),
    Cancel(JobId),
    Get(JobId),
    Snapshot(JobId),
    Snapshots(Vec<JobId>),
    Reap,
    Purge(DateTime<Utc>),
}
pub(crate) enum Outcome {
    Id(JobId),
    Lease(Option<LeasedJob>),
    Done,
    Canceled(bool),
    Record(JobRecord),
    Snapshot(crate::JobSnapshot),
    Snapshots(Vec<crate::JobSnapshot>),
    Reaped(Vec<ReapOutcome>),
    Purged(usize),
}

impl TenantState {
    #[cfg(test)]
    pub(crate) fn apply(&mut self, tenant: &str, operation: &Operation) -> QueueResult<Outcome> {
        self.apply_at(tenant, operation, Utc::now())
    }

    pub(crate) fn apply_at(
        &mut self,
        tenant: &str,
        operation: &Operation,
        now: DateTime<Utc>,
    ) -> QueueResult<Outcome> {
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
                        record: {
                            let mut record = JobRecord::new(id.clone(), tenant, message.clone());
                            record.created_at = now;
                            record.updated_at = now;
                            record
                        },
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
                row.record.updated_at = now;
                row.token = Some(token.clone());
                Ok(Outcome::Lease(Some(LeasedJob::new(
                    row.record.clone(),
                    token,
                    until,
                ))))
            }
            Operation::Snapshots(ids) => {
                let snapshots = ids
                    .iter()
                    .map(|id| {
                        self.jobs
                            .get(id)
                            .map(|s| crate::JobSnapshot::from(&s.record))
                            .ok_or_else(|| QueueError::JobNotFound(id.clone()))
                    })
                    .collect::<QueueResult<Vec<_>>>()?;
                Ok(Outcome::Snapshots(snapshots))
            }
            Operation::Snapshot(id) => Ok(Outcome::Snapshot(crate::JobSnapshot::from(
                &self
                    .jobs
                    .get(id)
                    .ok_or_else(|| QueueError::JobNotFound(id.clone()))?
                    .record,
            ))),
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
                row.record.status = JobStatus::Canceled { canceled_at: now };
                row.record.updated_at = now;
                row.record.lease_token = None;
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
                        row.record.status = JobStatus::Failed {
                            failed_at: now,
                            error: "Lease expired".into(),
                        };
                    }
                    row.record.updated_at = now;
                    row.record.lease_token = None;
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
                        row.record.status = JobStatus::Completed { completed_at: now };
                        row.record.updated_at = now;
                        row.record.lease_token = None;
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
                            _ => {
                                row.record.status = JobStatus::Failed {
                                    failed_at: now,
                                    error,
                                }
                            }
                        }
                        row.record.updated_at = now;
                        row.record.lease_token = None;
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
                        row.record.updated_at = now;
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
        Ok(self.get_snapshot(ctx, id).await?.status)
    }
    async fn get_snapshots(
        &self,
        ctx: QueueCtx,
        ids: &[JobId],
    ) -> QueueResult<Vec<crate::JobSnapshot>> {
        if ids.len() > 1000 {
            return Err(QueueError::InvalidConfig(
                "snapshot batches are limited to 1000 jobs".into(),
            ));
        }
        if ids.is_empty() {
            return Ok(vec![]);
        }
        match self
            .store
            .update(&ctx.tenant_id, &Operation::Snapshots(ids.to_vec()))
            .await?
        {
            Outcome::Snapshots(snapshots) => Ok(snapshots),
            _ => unreachable!(),
        }
    }
    async fn get_snapshot(&self, ctx: QueueCtx, id: JobId) -> QueueResult<crate::JobSnapshot> {
        match self
            .store
            .update(&ctx.tenant_id, &Operation::Snapshot(id))
            .await?
        {
            Outcome::Snapshot(snapshot) => Ok(snapshot),
            _ => unreachable!(),
        }
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

impl<S: StateStore> super::broker::JobLedger for DurableBackend<S> {}

#[cfg(test)]
mod latency_tests {
    use super::*;
    use crate::{Job, JobError, QueueAdapter, QueueConfig};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct SlowStore {
        state: tokio::sync::Mutex<TenantState>,
    }
    #[async_trait]
    impl StateStore for SlowStore {
        async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
            if matches!(op, Operation::Heartbeat(..)) {
                // Response/lock latency must not accumulate into a shrinking lease.
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            self.state.lock().await.apply(tenant, op)
        }
        async fn tenants(&self) -> QueueResult<Vec<String>> {
            Ok(vec!["latency".into()])
        }
    }
    #[derive(Serialize, Deserialize)]
    struct SlowJob;
    #[async_trait]
    impl Job for SlowJob {
        type Context = Arc<AtomicUsize>;
        type Result = ();
        const JOB_TYPE: &'static str = "slow-job";
        const MAX_RETRIES: u32 = 0;
        async fn execute(&self, executions: Self::Context) -> Result<(), JobError> {
            executions.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(3)).await;
            Ok(())
        }
    }
    #[tokio::test]
    async fn heartbeat_accounts_for_backend_round_trip_time() {
        let lease = Duration::from_millis(800);
        let backend = DurableBackend {
            store: SlowStore {
                state: Default::default(),
            },
            lease_duration: lease,
        };
        let adapter = QueueAdapter::try_with_config(
            backend,
            QueueConfig {
                max_workers: 1,
                lease_duration: lease,
                heartbeat_interval: Duration::from_millis(100),
                ..Default::default()
            },
        )
        .unwrap();
        adapter.register_job::<SlowJob>().await.unwrap();
        let ctx = QueueCtx::new("latency");
        let id = adapter.enqueue(ctx.clone(), SlowJob).await.unwrap();
        let executions = Arc::new(AtomicUsize::new(0));
        let workers = adapter
            .start_workers(
                ctx.clone(),
                executions.clone(),
                vec![SlowJob::JOB_TYPE.into()],
            )
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                let status = adapter
                    .backend()
                    .get_status(ctx.clone(), id.clone())
                    .await
                    .unwrap();
                if status.is_terminal() {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        workers.shutdown().await.unwrap();
        assert!(
            matches!(result.unwrap(), JobStatus::Completed { .. }),
            "a healthy slow heartbeat must retain ownership"
        );
        assert_eq!(executions.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod authoritative_clock_tests {
    use super::*;
    #[test]
    fn lease_and_completion_use_supplied_clock_not_worker_wall_time() {
        let now = DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut state = TenantState::default();
        let mut message = JobMessage::new("clock", vec![], "json", "q");
        message.run_at = now;
        let Outcome::Id(id) = state
            .apply_at("clock", &Operation::Enqueue(message), now)
            .unwrap()
        else {
            panic!()
        };
        let Outcome::Lease(Some(job)) = state
            .apply_at(
                "clock",
                &Operation::Dequeue(vec!["q".into()], Duration::from_secs(10)),
                now,
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(job.lease_until, now + chrono::Duration::seconds(10));
        assert_eq!(job.record.created_at, now);
        state
            .apply_at(
                "clock",
                &Operation::Complete(id.clone(), job.lease_token, None),
                now + chrono::Duration::seconds(1),
            )
            .unwrap();
        let record = &state.jobs[&id].record;
        assert_eq!(record.updated_at, now + chrono::Duration::seconds(1));
        assert!(
            matches!(record.status,JobStatus::Completed{completed_at} if completed_at == now+chrono::Duration::seconds(1))
        );
    }
}
