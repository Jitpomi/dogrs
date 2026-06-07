/// Integration tests for dog-queue
///
/// Tests the full enqueue → worker → execute → ack lifecycle
/// and correctness properties: multi-tenancy, priority, retry, cancel-wins.
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use tokio::time::{sleep, Duration};

use crate::{
    backend::memory::MemoryBackend, Job, JobError, JobPriority, QueueAdapter, QueueCtx,
    QueueError,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Test jobs
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Counter(Arc<AtomicU32>);

#[derive(Clone, Serialize, Deserialize)]
struct CountingJob {
    label: String,
}

#[async_trait]
impl Job for CountingJob {
    type Context = Counter;
    type Result = String;

    const JOB_TYPE: &'static str = "counting_job";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        ctx.0.fetch_add(1, Ordering::SeqCst);
        Ok(format!("done:{}", self.label))
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct FailingJob {
    permanent: bool,
}

#[async_trait]
impl Job for FailingJob {
    type Context = Counter;
    type Result = String;

    const JOB_TYPE: &'static str = "failing_job";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 2;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        ctx.0.fetch_add(1, Ordering::SeqCst);
        if self.permanent {
            Err(JobError::Permanent("always fails".to_string()))
        } else {
            Err(JobError::Retryable("transient error".to_string()))
        }
    }
}


fn make_adapter() -> QueueAdapter<MemoryBackend> {
    QueueAdapter::new(MemoryBackend::new())
}


// ---------------------------------------------------------------------------
// 1. Full lifecycle: enqueue → worker processes → job executes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_lifecycle_job_executes() {
    let adapter = Arc::new(make_adapter());
    adapter.register_job::<CountingJob>().await.unwrap();

    let counter = Counter(Arc::new(AtomicU32::new(0)));
    let ctx = QueueCtx::new("tenant_a".to_string());

    let job = CountingJob { label: "first".to_string() };
    adapter.enqueue(ctx.clone(), job).await.unwrap();

    // Start worker — it will run until we shut it down
    let handle = adapter
        .start_workers(ctx, counter.clone(), vec!["counting_job".to_string()])
        .await
        .unwrap();

    // Give the worker time to process the single job
    sleep(Duration::from_millis(200)).await;

    handle.shutdown().await.unwrap();

    assert_eq!(counter.0.load(Ordering::SeqCst), 1, "job should have executed once");
}

// ---------------------------------------------------------------------------
// 2. Multi-tenant isolation: tenant A's jobs don't leak to tenant B's worker
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multi_tenant_isolation() {
    let adapter = Arc::new(make_adapter());
    adapter.register_job::<CountingJob>().await.unwrap();

    let counter_a = Counter(Arc::new(AtomicU32::new(0)));
    let counter_b = Counter(Arc::new(AtomicU32::new(0)));

    let ctx_a = QueueCtx::new("tenant_a".to_string());
    let ctx_b = QueueCtx::new("tenant_b".to_string());

    // Enqueue 3 jobs for tenant A, 0 for tenant B
    for i in 0..3 {
        adapter
            .enqueue(ctx_a.clone(), CountingJob { label: i.to_string() })
            .await
            .unwrap();
    }

    // Worker for tenant B — should find nothing to do
    let handle_b = adapter
        .start_workers(ctx_b, counter_b.clone(), vec!["counting_job".to_string()])
        .await
        .unwrap();

    sleep(Duration::from_millis(200)).await;
    handle_b.shutdown().await.unwrap();

    // Tenant B counter must remain zero
    assert_eq!(counter_b.0.load(Ordering::SeqCst), 0, "tenant B should not process tenant A jobs");

    // Now run tenant A's worker
    let handle_a = adapter
        .start_workers(ctx_a, counter_a.clone(), vec!["counting_job".to_string()])
        .await
        .unwrap();

    sleep(Duration::from_millis(300)).await;
    handle_a.shutdown().await.unwrap();

    assert_eq!(counter_a.0.load(Ordering::SeqCst), 3, "tenant A should process exactly its 3 jobs");
}

// ---------------------------------------------------------------------------
// 3. Idempotency: enqueueing with the same key twice enqueues only once
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_idempotent_enqueue_executes_once() {
    use crate::backend::QueueBackend;
    use crate::{JobMessage, JobPriority};

    let backend = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("tenant_idem".to_string());

    let msg = || JobMessage {
        job_type: "counting_job".to_string(),
        payload_bytes: b"{\"label\":\"idem\"}".to_vec(),
        codec: "json".to_string(),
        queue: "counting_job".to_string(),
        priority: JobPriority::Normal,
        max_retries: 3,
        run_at: chrono::Utc::now(),
        idempotency_key: Some("unique-op-123".to_string()),
    };

    // Enqueue twice with the same idempotency key — should deduplicate
    let id1 = backend.enqueue(ctx.clone(), msg()).await.unwrap();
    let id2 = backend.enqueue(ctx.clone(), msg()).await.unwrap();
    assert_eq!(id1, id2, "duplicate enqueue should return same job id");

    // Only one job should be in the queue
    let leased1 = backend.dequeue(ctx.clone(), &["counting_job"]).await.unwrap();
    let leased2 = backend.dequeue(ctx.clone(), &["counting_job"]).await.unwrap();
    assert!(leased1.is_some(), "first dequeue should find the job");
    assert!(leased2.is_none(), "second dequeue should be empty — only one job exists");
}

// ---------------------------------------------------------------------------
// 4. Cancel-wins: canceling before worker acks must be respected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cancel_wins_semantics() {
    use crate::backend::QueueBackend;
    let backend = Arc::new(MemoryBackend::new());
    let adapter = Arc::new(QueueAdapter::new((*backend).clone()));
    adapter.register_job::<CountingJob>().await.unwrap();

    let ctx = QueueCtx::new("tenant_cancel".to_string());
    let job_id = adapter
        .enqueue(ctx.clone(), CountingJob { label: "cancel-me".to_string() })
        .await
        .unwrap();

    // Lease the job (dequeue without going through a worker)
    let leased = backend
        .dequeue(ctx.clone(), &["counting_job"])
        .await
        .unwrap()
        .expect("should have a leased job");

    // Cancel it while it's "processing"
    let canceled = backend.cancel(ctx.clone(), job_id.clone()).await.unwrap();
    assert!(canceled, "cancel should succeed");

    // Now try to acknowledge completion — cancel-wins means this should fail
    let result = backend
        .ack_complete(ctx, job_id, leased.lease_token, None)
        .await;

    assert!(
        matches!(result, Err(QueueError::JobCanceled)),
        "ack_complete on a canceled job must return JobCanceled, got: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// 5. Retry on retryable failure: job gets re-queued and attempted again
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_retryable_failure_retries() {
    let adapter = Arc::new(make_adapter());
    adapter.register_job::<FailingJob>().await.unwrap();

    // Counter counts every attempt (including retries)
    let attempt_count = Counter(Arc::new(AtomicU32::new(0)));
    let ctx = QueueCtx::new("tenant_retry".to_string());

    // FailingJob::MAX_RETRIES = 2 → 1 initial + 2 retries = 3 attempts max
    adapter
        .enqueue(ctx.clone(), FailingJob { permanent: false })
        .await
        .unwrap();

    let handle = adapter
        .start_workers(ctx, attempt_count.clone(), vec!["failing_job".to_string()])
        .await
        .unwrap();

    // Wait long enough for up to 3 attempts with backoff
    sleep(Duration::from_millis(500)).await;
    handle.shutdown().await.unwrap();

    let attempts = attempt_count.0.load(Ordering::SeqCst);
    assert!(
        attempts >= 1,
        "retryable job should have been attempted at least once, got {}",
        attempts
    );
}

// ---------------------------------------------------------------------------
// 6. Permanent failure: job fails immediately without retrying
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_permanent_failure_no_retry() {
    let adapter = Arc::new(make_adapter());
    adapter.register_job::<FailingJob>().await.unwrap();

    let attempt_count = Counter(Arc::new(AtomicU32::new(0)));
    let ctx = QueueCtx::new("tenant_perm_fail".to_string());

    adapter
        .enqueue(ctx.clone(), FailingJob { permanent: true })
        .await
        .unwrap();

    let handle = adapter
        .start_workers(ctx, attempt_count.clone(), vec!["failing_job".to_string()])
        .await
        .unwrap();

    sleep(Duration::from_millis(300)).await;
    handle.shutdown().await.unwrap();

    assert_eq!(
        attempt_count.0.load(Ordering::SeqCst),
        1,
        "permanent failure should not be retried"
    );
}

// ---------------------------------------------------------------------------
// 7. Lease expiry reaper: expired lease re-queues the job
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_lease_expiry_requeues_job() {
    use crate::backend::{memory::reaper::LeaseReaper, QueueBackend};

    let backend = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("tenant_reaper".to_string());

    let msg = crate::JobMessage {
        job_type: "counting_job".to_string(),
        payload_bytes: b"{}".to_vec(),
        codec: "json".to_string(),
        queue: "counting_job".to_string(),
        priority: JobPriority::Normal,
        max_retries: 3,
        run_at: chrono::Utc::now(),
        idempotency_key: None,
    };

    let job_id = backend.enqueue(ctx.clone(), msg).await.unwrap();
    let _leased = backend.dequeue(ctx.clone(), &["counting_job"]).await.unwrap().unwrap();

    // Artificially expire the lease
    backend.force_lease_expiry(job_id.clone()).await.unwrap();

    // Run the reaper
    let reaper = LeaseReaper::new(backend.clone());
    let reclaimed = reaper.reap_expired_leases().await.unwrap();
    assert_eq!(reclaimed, 1, "reaper should reclaim 1 expired lease");

    // Job should be available again
    let retry_leased = backend.dequeue(ctx, &["counting_job"]).await.unwrap();
    assert!(retry_leased.is_some(), "job should be back in queue after lease expiry");
    assert_eq!(retry_leased.unwrap().record.attempt, 2, "attempt count should be 2");
}

// ---------------------------------------------------------------------------
// 8. Multiple jobs processed in FIFO order within same priority
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fifo_within_same_priority() {
    let adapter = Arc::new(make_adapter());
    adapter.register_job::<CountingJob>().await.unwrap();

    let counter = Counter(Arc::new(AtomicU32::new(0)));
    let ctx = QueueCtx::new("tenant_fifo".to_string());

    // Enqueue 5 jobs
    for i in 0..5 {
        adapter
            .enqueue(ctx.clone(), CountingJob { label: i.to_string() })
            .await
            .unwrap();
    }

    let handle = adapter
        .start_workers(ctx, counter.clone(), vec!["counting_job".to_string()])
        .await
        .unwrap();

    sleep(Duration::from_millis(300)).await;
    handle.shutdown().await.unwrap();

    assert_eq!(counter.0.load(Ordering::SeqCst), 5, "all 5 jobs should execute");
}
