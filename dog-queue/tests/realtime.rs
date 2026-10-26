use dog_queue::{
    backend::memory::MemoryBackend,
    realtime::{watch_jobs, WatchOptions},
    JobMessage, QueueBackend, QueueCtx,
};
use futures::StreamExt;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn snapshots_reconcile_after_disconnect_and_do_not_leak_payloads() {
    let backend = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("tenant-a");
    let id = backend
        .enqueue(
            ctx.clone(),
            JobMessage::new("secret-job", b"secret-payload".to_vec(), "bytes", "q"),
        )
        .await
        .unwrap();
    let mut stream = watch_jobs(
        backend.clone(),
        ctx.clone(),
        vec![id.clone()],
        WatchOptions::default(),
    )
    .unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap().status, "enqueued");
    drop(stream);
    let leased = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
    backend
        .ack_complete(
            ctx.clone(),
            id.clone(),
            leased.lease_token,
            Some("secret-result".into()),
        )
        .await
        .unwrap();
    let mut reconnected = watch_jobs(
        backend.clone(),
        ctx,
        vec![id.clone()],
        WatchOptions::default(),
    )
    .unwrap();
    let update = reconnected.next().await.unwrap().unwrap();
    assert_eq!(update.status, "completed");
    let json = serde_json::to_string(&update).unwrap();
    assert!(!json.contains("secret"));
    let mut wrong_tenant = watch_jobs(
        backend,
        QueueCtx::new("tenant-b"),
        vec![id],
        WatchOptions::default(),
    )
    .unwrap();
    assert!(wrong_tenant.next().await.unwrap().is_err());
    assert!(wrong_tenant.next().await.is_none());
}

#[tokio::test(start_paused = true)]
async fn bounded_lifetime_and_deduplication() {
    let backend = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("a");
    let id = backend
        .enqueue(ctx.clone(), JobMessage::new("test", vec![], "bytes", "q"))
        .await
        .unwrap();
    let options = WatchOptions {
        lifetime: Duration::from_secs(2),
        ..Default::default()
    };
    let mut stream = watch_jobs(
        backend.clone(),
        ctx.clone(),
        vec![id.clone(), id.clone()],
        options,
    )
    .unwrap();
    assert!(stream.next().await.unwrap().is_ok());
    assert!(stream.next().await.is_none());
    assert!(watch_jobs(
        backend,
        ctx,
        vec![id],
        WatchOptions {
            interval: Duration::ZERO,
            ..Default::default()
        }
    )
    .is_err());
}

struct Stalled;
#[async_trait::async_trait]
impl QueueBackend for Stalled {
    async fn get_snapshot(
        &self,
        _: QueueCtx,
        _: dog_queue::JobId,
    ) -> dog_queue::QueueResult<dog_queue::JobSnapshot> {
        std::future::pending().await
    }
    async fn enqueue(
        &self,
        _: QueueCtx,
        _: JobMessage,
    ) -> dog_queue::QueueResult<dog_queue::JobId> {
        unreachable!()
    }
    async fn dequeue(
        &self,
        _: QueueCtx,
        _: &[&str],
    ) -> dog_queue::QueueResult<Option<dog_queue::LeasedJob>> {
        unreachable!()
    }
    async fn ack_complete(
        &self,
        _: QueueCtx,
        _: dog_queue::JobId,
        _: dog_queue::LeaseToken,
        _: Option<String>,
    ) -> dog_queue::QueueResult<()> {
        unreachable!()
    }
    async fn ack_fail(
        &self,
        _: QueueCtx,
        _: dog_queue::JobId,
        _: dog_queue::LeaseToken,
        _: String,
        _: Option<chrono::DateTime<chrono::Utc>>,
    ) -> dog_queue::QueueResult<()> {
        unreachable!()
    }
    async fn cancel(&self, _: QueueCtx, _: dog_queue::JobId) -> dog_queue::QueueResult<bool> {
        unreachable!()
    }
    async fn get_status(
        &self,
        _: QueueCtx,
        _: dog_queue::JobId,
    ) -> dog_queue::QueueResult<dog_queue::JobStatus> {
        unreachable!()
    }
    fn event_stream(&self, _: QueueCtx) -> dog_queue::backend::BoxStream<dog_queue::JobEvent> {
        unreachable!()
    }
    fn capabilities(&self) -> dog_queue::QueueCapabilities {
        unreachable!()
    }
}
#[tokio::test(start_paused = true)]
async fn stalled_backend_is_bounded_and_stream_ends_after_error() {
    let mut stream = watch_jobs(
        Arc::new(Stalled),
        QueueCtx::new("a"),
        vec![dog_queue::JobId::new()],
        WatchOptions {
            read_timeout: Duration::from_secs(2),
            ..Default::default()
        },
    )
    .unwrap();
    let started = tokio::time::Instant::now();
    assert!(stream.next().await.unwrap().is_err());
    assert_eq!(started.elapsed(), Duration::from_secs(2));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn progress_is_fenced_and_results_require_opt_in() {
    let b = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("owner");
    let id = b
        .enqueue(ctx.clone(), JobMessage::new("j", vec![], "json", "q"))
        .await
        .unwrap();
    let lease = b.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
    b.report_progress(ctx.clone(), id.clone(), lease.lease_token.clone(), 40)
        .await
        .unwrap();
    assert!(b
        .report_progress(
            QueueCtx::new("other"),
            id.clone(),
            lease.lease_token.clone(),
            90
        )
        .await
        .is_err());
    assert!(b
        .report_progress(ctx.clone(), id.clone(), dog_queue::LeaseToken::new(), 90)
        .await
        .is_err());
    assert!(b
        .report_progress(ctx.clone(), id.clone(), lease.lease_token.clone(), 101)
        .await
        .is_err());
    let mut s = watch_jobs(
        b.clone(),
        ctx.clone(),
        vec![id.clone()],
        WatchOptions::default(),
    )
    .unwrap();
    assert_eq!(s.next().await.unwrap().unwrap().progress, Some(40));
    b.ack_complete(
        ctx.clone(),
        id.clone(),
        lease.lease_token.clone(),
        Some("private-result".into()),
    )
    .await
    .unwrap();
    assert!(b
        .report_progress(ctx.clone(), id.clone(), lease.lease_token, 90)
        .await
        .is_err());
    let mut private = watch_jobs(
        b.clone(),
        ctx.clone(),
        vec![id.clone()],
        WatchOptions::default(),
    )
    .unwrap();
    assert!(private.next().await.unwrap().unwrap().result.is_none());
    let mut allowed = watch_jobs(
        b,
        ctx,
        vec![id],
        WatchOptions {
            include_results: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        allowed.next().await.unwrap().unwrap().result.as_deref(),
        Some("private-result")
    );
    assert!(dog_queue::realtime::report_progress(1).await.is_err());
}
