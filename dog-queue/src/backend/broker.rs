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
        Arc, OnceLock,
    },
    time::Duration,
};

/// Persistent job state; implemented by the PostgreSQL, Redis and JetStream ledgers.
/// Redis and JetStream must be provisioned with persistence and suitable replication.
/// Custom ledgers may implement this public extension point. They must durably
/// commit enqueue before returning success, atomically claim jobs across workers,
/// fence expired/replaced lease owners, isolate tenants, and retain recoverable
/// state independently of wakeup delivery. Implementing this marker is an explicit
/// assertion of those guarantees; it does not turn an in-memory store durable.
pub trait JobLedger: QueueBackend {}

#[async_trait]
pub trait Notifications: Send + Sync {
    /// Publish an opaque wakeup. No customer data should be sent to the broker.
    async fn publish(&self) -> QueueResult<()>;
    /// Wait for and acknowledge one wakeup. Cancellation must be safe.
    async fn receive(&self) -> QueueResult<bool>;
    /// Release adapter-owned receive resources after background tasks stop.
    async fn shutdown(&self) {}
}

pub struct BrokerBackend<N> {
    ledger: Arc<dyn JobLedger>,
    notifications: Arc<N>,
    publish_failures: Arc<AtomicU64>,
    receive_failures: Arc<AtomicU64>,
    receive_timeouts: Arc<AtomicU64>,
    wake: Arc<tokio::sync::Notify>,
    signal: Arc<tokio::sync::Notify>,
    tasks: OnceLock<Vec<tokio::task::JoinHandle<()>>>,
}
impl<N: Notifications + 'static> BrokerBackend<N> {
    pub fn with_ledger(notifications: N, ledger: Arc<dyn JobLedger>) -> Self {
        Self {
            ledger,
            notifications: Arc::new(notifications),
            publish_failures: Arc::new(AtomicU64::new(0)),
            receive_failures: Arc::new(AtomicU64::new(0)),
            receive_timeouts: Arc::new(AtomicU64::new(0)),
            wake: Arc::new(tokio::sync::Notify::new()),
            signal: Arc::new(tokio::sync::Notify::new()),
            tasks: OnceLock::new(),
        }
    }
    /// Notification errors do not lose committed jobs; expose degradation to monitoring.
    pub fn notification_failures(&self) -> (u64, u64) {
        (
            self.publish_failures.load(Ordering::Relaxed),
            self.receive_failures.load(Ordering::Relaxed),
        )
    }
    /// Receive deadline expirations, including idle streaming subscriptions.
    /// Kept separate from explicit transport errors.
    pub fn notification_receive_timeouts(&self) -> u64 {
        self.receive_timeouts.load(Ordering::Relaxed)
    }
    pub fn notifications(&self) -> &N {
        &self.notifications
    }
    // Start lazily: constructing a backend does not require an active runtime.
    fn start_notifications(&self) {
        self.tasks.get_or_init(|| {
            let notifications = self.notifications.clone();
            let failures = self.publish_failures.clone();
            let signal = self.signal.clone();
            let publisher = tokio::spawn(async move {
                loop {
                    signal.notified().await;
                    if !matches!(
                        tokio::time::timeout(Duration::from_secs(2), notifications.publish()).await,
                        Ok(Ok(()))
                    ) {
                        failures.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!("Broker publish failed; job remains in ledger");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            });
            let notifications = self.notifications.clone();
            let failures = self.receive_failures.clone();
            let timeouts = self.receive_timeouts.clone();
            let wake = self.wake.clone();
            let receiver = tokio::spawn(async move {
                loop {
                    match tokio::time::timeout(Duration::from_secs(30), notifications.receive())
                        .await
                    {
                        Ok(Ok(true)) => {
                            wake.notify_waiters();
                            tokio::task::yield_now().await;
                        }
                        Ok(Ok(false)) => tokio::time::sleep(Duration::from_millis(25)).await,
                        Ok(Err(error)) => {
                            failures.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!(%error, "Broker receive failed; ledger polling remains available");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(_) => {
                            timeouts.fetch_add(1, Ordering::Relaxed);
                            // Streaming receivers can legitimately be idle.
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    }
                }
            });
            vec![publisher, receiver]
        });
    }
    fn signal(&self) {
        self.start_notifications();
        // At most one pending wakeup, in addition to the in-flight publication.
        self.signal.notify_one();
    }
    /// Stop wrapper tasks and release receive resources. Durable jobs are unaffected.
    /// SDK connections and already queued producer requests may outlive this call.
    /// Subsequent enqueue/dequeue calls restart notification tasks.
    pub async fn shutdown_notifications(&mut self) {
        if let Some(tasks) = self.tasks.take() {
            for task in &tasks {
                task.abort();
            }
            for task in tasks {
                let _ = task.await;
            }
        }
        self.notifications.shutdown().await;
    }
}
impl<N> Drop for BrokerBackend<N> {
    fn drop(&mut self) {
        if let Some(tasks) = self.tasks.get() {
            for task in tasks {
                task.abort();
            }
        }
    }
}

#[async_trait]
impl<N: Notifications + 'static> QueueBackend for BrokerBackend<N> {
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId> {
        let id = self.ledger.enqueue(ctx, message).await?;
        self.signal();
        Ok(id)
    }
    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>> {
        self.start_notifications();
        let notified = self.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if let Some(job) = self.ledger.dequeue(ctx.clone(), queues).await? {
            return Ok(Some(job));
        }
        // Subscribe before the first lookup. A missing hint only delays the
        // next lookup by this bounded poll interval; ready jobs never wait.
        let _ = tokio::time::timeout(Duration::from_millis(50), notified).await;
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
            self.signal();
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
    async fn get_snapshots(
        &self,
        ctx: QueueCtx,
        ids: &[JobId],
    ) -> QueueResult<Vec<crate::JobSnapshot>> {
        self.ledger.get_snapshots(ctx, ids).await
    }
    async fn get_snapshot(&self, ctx: QueueCtx, id: JobId) -> QueueResult<crate::JobSnapshot> {
        self.ledger.get_snapshot(ctx, id).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::memory::MemoryBackend;
    // Test-only ledger: never exposed as a production durability implementation.
    impl JobLedger for MemoryBackend {}
    struct Hanging;
    #[async_trait]
    impl Notifications for Hanging {
        async fn publish(&self) -> QueueResult<()> {
            std::future::pending().await
        }
        async fn receive(&self) -> QueueResult<bool> {
            std::future::pending().await
        }
    }
    #[tokio::test]
    async fn shutdown_cleans_up_before_start_and_after_restart() {
        struct Cleanup(Arc<AtomicU64>);
        #[async_trait]
        impl Notifications for Cleanup {
            async fn publish(&self) -> QueueResult<()> {
                Ok(())
            }
            async fn receive(&self) -> QueueResult<bool> {
                std::future::pending().await
            }
            async fn shutdown(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let count = Arc::new(AtomicU64::new(0));
        let mut backend =
            BrokerBackend::with_ledger(Cleanup(count.clone()), Arc::new(MemoryBackend::new()));
        backend.shutdown_notifications().await;
        backend.shutdown_notifications().await;
        assert_eq!(count.load(Ordering::SeqCst), 2);
        backend.start_notifications();
        backend.shutdown_notifications().await;
        backend.start_notifications();
        backend.shutdown_notifications().await;
        assert_eq!(count.load(Ordering::SeqCst), 4);
    }
    #[tokio::test]
    async fn unavailable_notifications_cannot_delay_ready_jobs() {
        let mut backend = BrokerBackend::with_ledger(Hanging, Arc::new(MemoryBackend::new()));
        let ctx = QueueCtx::new("notification-outage");
        tokio::time::timeout(Duration::from_millis(40), async {
            let id = backend
                .enqueue(ctx.clone(), JobMessage::new("test", vec![42], "bytes", "q"))
                .await
                .unwrap();
            let job = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
            assert_eq!(id, job.record.job_id);
            backend
                .ack_complete(ctx, id, job.lease_token, None)
                .await
                .unwrap();
        })
        .await
        .expect("notification transport must not be on the job path");
        backend.shutdown_notifications().await;
        assert!(backend.tasks.get().is_none());
    }
    #[tokio::test]
    async fn lost_wakeup_still_discovers_a_new_job() {
        let ledger = Arc::new(MemoryBackend::new());
        let backend = BrokerBackend::with_ledger(Hanging, ledger.clone());
        let producer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            ledger
                .enqueue(
                    QueueCtx::new("lost-hint"),
                    JobMessage::new("test", vec![], "bytes", "q"),
                )
                .await
                .unwrap()
        });
        let job = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(job) = backend
                    .dequeue(QueueCtx::new("lost-hint"), &["q"])
                    .await
                    .unwrap()
                {
                    break job;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(job.record.job_id, producer.await.unwrap());
    }
    #[tokio::test]
    async fn explicit_notification_errors_remain_observable() {
        struct Offline;
        #[async_trait]
        impl Notifications for Offline {
            async fn publish(&self) -> QueueResult<()> {
                Err(crate::QueueError::Internal("offline".into()))
            }
            async fn receive(&self) -> QueueResult<bool> {
                Err(crate::QueueError::Internal("offline".into()))
            }
        }
        let mut backend = BrokerBackend::with_ledger(Offline, Arc::new(MemoryBackend::new()));
        backend
            .enqueue(
                QueueCtx::new("errors"),
                JobMessage::new("test", vec![], "bytes", "q"),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while backend.notification_failures().0 == 0 || backend.notification_failures().1 == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(backend.notification_receive_timeouts(), 0);
        backend.shutdown_notifications().await;
    }
    #[tokio::test]
    async fn dropping_backend_releases_background_ownership() {
        let notifications = Arc::new(AtomicU64::new(0));
        struct Pending(Arc<AtomicU64>);
        #[async_trait]
        impl Notifications for Pending {
            async fn publish(&self) -> QueueResult<()> {
                let _ = &self.0;
                std::future::pending().await
            }
            async fn receive(&self) -> QueueResult<bool> {
                std::future::pending().await
            }
        }
        let backend = BrokerBackend::with_ledger(
            Pending(notifications.clone()),
            Arc::new(MemoryBackend::new()),
        );
        backend.start_notifications();
        tokio::task::yield_now().await;
        drop(backend);
        tokio::time::timeout(Duration::from_secs(1), async {
            while Arc::strong_count(&notifications) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
