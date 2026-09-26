//! Broker notifications combined with an authoritative durable job ledger.
//!
//! Notifications carry no job payload and may be duplicated or lost. Workers always
//! poll the ledger too, so a crash between its commit and publication cannot lose work.
//! Only `ack_complete` commits job completion. Acknowledging a notification never does.
use super::{BoxStream, QueueBackend, ReapOutcome};
use crate::{
    types::LeaseToken, JobEvent, JobId, JobMessage, JobRecord, JobStatus, LeasedJob,
    QueueCapabilities, QueueCtx, QueueResult,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

pub(crate) mod sealed {
    pub trait Sealed {}
}
/// Persistent job state; implemented by the PostgreSQL, Redis and JetStream ledgers.
/// Redis and JetStream must be provisioned with persistence and suitable replication.
pub trait JobLedger: QueueBackend + sealed::Sealed {}

#[async_trait]
pub trait Notifications: Send + Sync {
    /// Publish an opaque wakeup. No customer data should be sent to the broker.
    async fn publish(&self) -> QueueResult<()>;
    /// Wait for and acknowledge one wakeup. Cancellation must be safe.
    async fn receive(&self) -> QueueResult<bool>;
}

pub struct BrokerBackend<N> {
    ledger: Arc<dyn JobLedger>,
    notifications: N,
    publish_failures: AtomicU64,
    receive_failures: AtomicU64,
}
impl<N: Notifications> BrokerBackend<N> {
    pub fn with_ledger(notifications: N, ledger: Arc<dyn JobLedger>) -> Self {
        Self {
            ledger,
            notifications,
            publish_failures: AtomicU64::new(0),
            receive_failures: AtomicU64::new(0),
        }
    }
    /// Notification errors do not lose committed jobs; expose degradation to monitoring.
    pub fn notification_failures(&self) -> (u64, u64) {
        (
            self.publish_failures.load(Ordering::Relaxed),
            self.receive_failures.load(Ordering::Relaxed),
        )
    }
    pub fn notifications(&self) -> &N {
        &self.notifications
    }
    async fn signal(&self) {
        if !matches!(
            tokio::time::timeout(Duration::from_secs(2), self.notifications.publish()).await,
            Ok(Ok(()))
        ) {
            self.publish_failures.fetch_add(1, Ordering::Relaxed);
            tracing::warn!("Broker notification failed; committed job remains available through ledger polling");
        }
    }
}
#[async_trait]
impl<N: Notifications> QueueBackend for BrokerBackend<N> {
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
        let id = self.ledger.enqueue(ctx, message).await?;
        self.signal().await;
        Ok(id)
    }
    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
        // Drain at most one hint per call, including when the ledger has ready jobs.
        // Bounded polling keeps retries and scheduled jobs moving during broker outages.
        if let Ok(Err(err)) =
            tokio::time::timeout(Duration::from_millis(50), self.notifications.receive()).await
        {
            self.receive_failures.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(%err, "Broker receive failed; polling durable ledger");
        }
        self.ledger.dequeue(ctx, queues).await
    }
    async fn ack_complete(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        result: Option<String>,
    ) -> QueueResult<()> {
        self.ledger.ack_complete(ctx, id, token, result).await
    }
    async fn ack_fail(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        error: String,
        retry: Option<DateTime<Utc>>,
    ) -> QueueResult<()> {
        self.ledger.ack_fail(ctx, id, token, error, retry).await?;
        if retry.is_some() {
            self.signal().await;
        }
        Ok(())
    }
    async fn heartbeat_extend(
        &self,
        ctx: QueueCtx,
        id: JobId,
        token: LeaseToken,
        duration: Duration,
    ) -> QueueResult<()> {
        self.ledger.heartbeat_extend(ctx, id, token, duration).await
    }
    async fn cancel(&self, ctx: QueueCtx, id: JobId) -> QueueResult<bool> {
        self.ledger.cancel(ctx, id).await
    }
    async fn get_status(&self, ctx: QueueCtx, id: JobId) -> QueueResult<JobStatus> {
        self.ledger.get_status(ctx, id).await
    }
    async fn get_record(&self, ctx: QueueCtx, id: JobId) -> QueueResult<JobRecord> {
        self.ledger.get_record(ctx, id).await
    }
    fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent> {
        self.ledger.event_stream(ctx)
    }
    async fn reclaim_expired_leases(&self) -> QueueResult<Vec<ReapOutcome>> {
        self.ledger.reclaim_expired_leases().await
    }
    fn capabilities(&self) -> QueueCapabilities {
        self.ledger.capabilities()
    }
}
