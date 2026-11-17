//! Durable recurrence independent of application jobs and storage providers.
//! A backend must atomically advance the cursor and persist the occurrence delivery.
use crate::{Job, JobError, JobMessage, JobPriority, QueueCtx, QueueError, QueueResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntervalCursor {
    pub every_seconds: i64,
    pub next: i64,
    pub sequence: u64,
}
impl IntervalCursor {
    pub fn validate(&self) -> QueueResult<()> {
        if self.every_seconds <= 0 || DateTime::from_timestamp(self.next, 0).is_none() {
            return Err(QueueError::InvalidConfig(
                "Invalid interval or UTC cursor".into(),
            ));
        }
        Ok(())
    }
    /// Fixed-rate catch-up: never silently skips an occurrence after downtime.
    pub fn advance(&self) -> QueueResult<Self> {
        self.validate()?;
        let next = self
            .next
            .checked_add(self.every_seconds)
            .ok_or_else(|| QueueError::InvalidConfig("Schedule time exhausted".into()))?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| QueueError::InvalidConfig("Schedule sequence exhausted".into()))?;
        let result = Self {
            every_seconds: self.every_seconds,
            next,
            sequence,
        };
        result.validate()?;
        Ok(result)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Occurrence {
    pub schedule_id: String,
    pub sequence: u64,
    pub scheduled_at: i64,
}
impl Occurrence {
    /// Tenant scoping is supplied separately by QueueCtx. Stable across restarts.
    pub fn key(&self) -> String {
        format!("schedule:{}:{}", self.schedule_id, self.sequence)
    }
}
#[async_trait]
pub trait ScheduledJob: Send + Sync + Serialize + serde::de::DeserializeOwned + 'static {
    type Context: Send + Sync + Clone + 'static;
    type Result: Send + Serialize + serde::de::DeserializeOwned + 'static;
    const JOB_TYPE: &'static str;
    const MAX_RETRIES: u32 = 3;
    async fn execute(
        &self,
        context: Self::Context,
        occurrence: Occurrence,
    ) -> Result<Self::Result, JobError>;
}
#[derive(Serialize, Deserialize)]
pub struct Scheduled<J> {
    pub occurrence: Occurrence,
    pub job: J,
}
#[async_trait]
impl<J: ScheduledJob> Job for Scheduled<J> {
    type Context = J::Context;
    type Result = J::Result;
    const JOB_TYPE: &'static str = J::JOB_TYPE;
    const MAX_RETRIES: u32 = J::MAX_RETRIES;
    async fn execute(&self, context: Self::Context) -> Result<Self::Result, JobError> {
        self.job.execute(context, self.occurrence.clone()).await
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub job_type: String,
    pub job: Value,
    pub queue: String,
    pub priority: JobPriority,
    pub max_retries: u32,
}
impl Template {
    pub fn new<J: ScheduledJob>(job: &J) -> QueueResult<Self> {
        Ok(Self {
            job_type: J::JOB_TYPE.into(),
            job: serde_json::to_value(job)?,
            queue: J::JOB_TYPE.into(),
            priority: JobPriority::Normal,
            max_retries: J::MAX_RETRIES,
        })
    }
    fn message(&self, occurrence: &Occurrence) -> QueueResult<JobMessage> {
        Ok(JobMessage::new(
            &self.job_type,
            serde_json::to_vec(&serde_json::json!({"occurrence":occurrence,"job":self.job}))?,
            "json",
            &self.queue,
        )
        .with_priority(self.priority)
        .with_max_retries(self.max_retries)
        .with_run_at(
            DateTime::from_timestamp(occurrence.scheduled_at, 0)
                .ok_or_else(|| QueueError::InvalidConfig("Invalid occurrence UTC time".into()))?,
        )
        .with_idempotency_key(occurrence.key()))
    }
}
#[derive(Clone, Debug)]
pub struct ScheduleRecord {
    pub id: String,
    pub revision: String,
    pub cursor: IntervalCursor,
    pub enabled: bool,
    pub template: Template,
}
/// A durable storage contract, not a timer in application code. On an uncertain
/// commit, do not blindly repeat it: reload the current revision on redelivery.
#[async_trait]
pub trait RecurrenceBackend: Send + Sync {
    async fn load(&self, ctx: &QueueCtx, id: &str) -> QueueResult<Option<ScheduleRecord>>;
    /// Atomically compare revision, persist occurrence, advance cursor and publish
    /// the next scheduler wake-up. A failed predicate must commit nothing.
    async fn commit(
        &self,
        ctx: &QueueCtx,
        old: &ScheduleRecord,
        next: &IntervalCursor,
        message: JobMessage,
    ) -> QueueResult<()>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum Dispatch {
    Obsolete,
    NotDue,
    Published(Occurrence),
}
pub async fn dispatch<B: RecurrenceBackend + ?Sized>(
    backend: &B,
    ctx: &QueueCtx,
    id: &str,
    revision: &str,
    now: DateTime<Utc>,
) -> QueueResult<Dispatch> {
    let Some(record) = backend.load(ctx, id).await? else {
        return Ok(Dispatch::Obsolete);
    };
    if record.id != id {
        return Err(QueueError::InvalidConfig(
            "Schedule identity mismatch".into(),
        ));
    }
    if record.revision != revision || !record.enabled {
        return Ok(Dispatch::Obsolete);
    }
    record.cursor.validate()?;
    if record.cursor.next > now.timestamp() {
        return Ok(Dispatch::NotDue);
    }
    let occurrence = Occurrence {
        schedule_id: record.id.clone(),
        sequence: record.cursor.sequence,
        scheduled_at: record.cursor.next,
    };
    let next = record.cursor.advance()?;
    backend
        .commit(ctx, &record, &next, record.template.message(&occurrence)?)
        .await?;
    Ok(Dispatch::Published(occurrence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Backend {
        state: Mutex<(ScheduleRecord, Vec<JobMessage>)>,
    }
    #[async_trait]
    impl RecurrenceBackend for Backend {
        async fn load(&self, ctx: &QueueCtx, _: &str) -> QueueResult<Option<ScheduleRecord>> {
            if ctx.tenant_id != "tenant" {
                return Err(QueueError::InvalidConfig("Foreign tenant".into()));
            }
            Ok(Some(self.state.lock().unwrap().0.clone()))
        }
        async fn commit(
            &self,
            _: &QueueCtx,
            old: &ScheduleRecord,
            next: &IntervalCursor,
            message: JobMessage,
        ) -> QueueResult<()> {
            let mut state = self.state.lock().unwrap();
            if state.0.revision != old.revision {
                return Err(QueueError::Internal("CAS lost".into()));
            }
            state.1.push(message);
            state.0.cursor = next.clone();
            state.0.revision = uuid::Uuid::new_v4().to_string();
            Ok(())
        }
    }
    #[tokio::test]
    async fn durable_catch_up_and_stale_delivery_are_generic() {
        let b = Backend {
            state: Mutex::new((
                ScheduleRecord {
                    id: "s".into(),
                    revision: "old".into(),
                    cursor: IntervalCursor {
                        every_seconds: 60,
                        next: 100,
                        sequence: 7,
                    },
                    enabled: true,
                    template: Template {
                        job_type: "any.job".into(),
                        job: serde_json::json!({"arbitrary":"payload"}),
                        queue: "q".into(),
                        priority: JobPriority::Normal,
                        max_retries: 3,
                    },
                },
                vec![],
            )),
        };
        let ctx = QueueCtx::new("tenant");
        let now = DateTime::from_timestamp(500, 0).unwrap();
        assert!(matches!(
            dispatch(&b, &ctx, "s", "old", now).await.unwrap(),
            Dispatch::Published(_)
        ));
        assert_eq!(
            dispatch(&b, &ctx, "s", "old", now).await.unwrap(),
            Dispatch::Obsolete
        );
        let record = b.load(&ctx, "s").await.unwrap().unwrap();
        assert_eq!(record.cursor.next, 160);
        assert_eq!(record.cursor.sequence, 8);
        dispatch(&b, &ctx, "s", &record.revision, now)
            .await
            .unwrap();
        let state = b.state.lock().unwrap();
        assert_eq!(state.1.len(), 2);
        assert_eq!(state.1[0].idempotency_key.as_deref(), Some("schedule:s:7"));
        assert_eq!(state.1[1].idempotency_key.as_deref(), Some("schedule:s:8"));
    }
    #[tokio::test]
    async fn concurrent_publish_and_lost_ack_never_duplicate_an_occurrence() {
        struct LostAck(Backend);
        #[async_trait]
        impl RecurrenceBackend for LostAck {
            async fn load(&self, ctx: &QueueCtx, id: &str) -> QueueResult<Option<ScheduleRecord>> {
                self.0.load(ctx, id).await
            }
            async fn commit(
                &self,
                ctx: &QueueCtx,
                old: &ScheduleRecord,
                next: &IntervalCursor,
                message: JobMessage,
            ) -> QueueResult<()> {
                self.0.commit(ctx, old, next, message).await?;
                Err(QueueError::Internal("lost commit acknowledgement".into()))
            }
        }
        let backend = LostAck(Backend {
            state: Mutex::new((
                ScheduleRecord {
                    id: "s".into(),
                    revision: "r".into(),
                    cursor: IntervalCursor {
                        every_seconds: 60,
                        next: 100,
                        sequence: 0,
                    },
                    enabled: true,
                    template: Template {
                        job_type: "arbitrary.job".into(),
                        job: serde_json::json!({}),
                        queue: "q".into(),
                        priority: JobPriority::Normal,
                        max_retries: 3,
                    },
                },
                vec![],
            )),
        });
        let ctx = QueueCtx::new("tenant");
        let now = DateTime::from_timestamp(500, 0).unwrap();
        let _ = tokio::join!(
            dispatch(&backend, &ctx, "s", "r", now),
            dispatch(&backend, &ctx, "s", "r", now)
        );
        assert_eq!(backend.0.state.lock().unwrap().1.len(), 1);
        assert_eq!(
            dispatch(&backend, &ctx, "s", "r", now).await.unwrap(),
            Dispatch::Obsolete
        );
        assert!(dispatch(&backend, &QueueCtx::new("other"), "s", "r", now)
            .await
            .is_err());
        let record = backend.load(&ctx, "s").await.unwrap().unwrap();
        assert_eq!(
            dispatch(
                &backend,
                &ctx,
                "s",
                &record.revision,
                DateTime::from_timestamp(120, 0).unwrap()
            )
            .await
            .unwrap(),
            Dispatch::NotDue
        );
        backend.0.state.lock().unwrap().0.enabled = false;
        assert_eq!(
            dispatch(&backend, &ctx, "s", &record.revision, now)
                .await
                .unwrap(),
            Dispatch::Obsolete
        );
    }
    #[test]
    fn exhaustion_is_an_error_without_partial_cursor_changes() {
        assert!(IntervalCursor {
            every_seconds: 60,
            next: 100,
            sequence: u64::MAX
        }
        .advance()
        .is_err());
        assert!(IntervalCursor {
            every_seconds: 0,
            next: 100,
            sequence: 0
        }
        .advance()
        .is_err());
    }
}
