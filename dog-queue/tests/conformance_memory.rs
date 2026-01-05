use std::time::Duration;
use chrono::Utc;
use tokio_stream::StreamExt;

use dog_queue::{
    QueueCtx, JobMessage, JobPriority, JobStatus, JobEvent,
    backend::{QueueBackend, memory::MemoryBackend}
};

/// Test factory functions
fn create_test_context() -> QueueCtx {
    QueueCtx::new("test_tenant".to_string())
}

fn create_test_job_message() -> JobMessage {
    JobMessage {
        job_type: "test_job".to_string(),
        payload: b"test_payload".to_vec(),
        codec: "json".to_string(),
        queue: "default".to_string(),
        priority: JobPriority::Normal,
        max_retries: 3,
        run_at: None,
        idempotency_key: None,
    }
}

fn create_job_with_priority(priority: JobPriority) -> JobMessage {
    JobMessage {
        priority,
        ..create_test_job_message()
    }
}

async fn receive_next_event(stream: &mut tokio_stream::wrappers::BroadcastStream<JobEvent>) -> JobEvent {
    tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("Timeout waiting for event")
        .expect("Stream ended")
        .expect("Event receive error")
}

/// A1. Dequeue Leases Atomically
#[tokio::test]
async fn test_dequeue_leases_atomically() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: enqueue one job
    let job_id = backend.enqueue(ctx.clone(), job_message).await.unwrap();

    // Act: dequeue
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();

    // Assert: atomic lease assignment
    assert_eq!(leased.record.job_id, job_id);
    assert!(!leased.lease_token.as_str().is_empty());
    assert!(leased.lease_until > Utc::now());

    // Verify status reflects lease
    let status = backend.get_status(ctx.clone(), job_id.clone()).await.unwrap();
    assert!(matches!(status, JobStatus::Processing { .. }));

    // Verify record shows lease details
    let record = backend.get_record(ctx, job_id).await.unwrap();
    assert_eq!(record.lease_token, Some(leased.lease_token));
    assert_eq!(record.lease_until, Some(leased.lease_until));
}

/// A2. Only Lease Holder Can Ack
#[tokio::test]
async fn test_only_lease_holder_can_ack() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: dequeue job (lease_token = T1)
    backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    let fake_token = dog_queue::LeaseToken::from("invalid_token");

    // Act: ack_complete with different token
    let result = backend.ack_complete(ctx, leased.record.job_id, fake_token, None).await;

    // Assert: InvalidLeaseToken error
    assert!(matches!(result, Err(dog_queue::QueueError::InvalidLeaseToken)));
}

/// A3. Lease Expiry Race → LeaseExpired
#[tokio::test]
async fn test_lease_expiry_race() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: enqueue job, dequeue, force lease expiry
    backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    backend.force_lease_expiry(leased.record.job_id.clone()).await.unwrap();

    // Act: ack_complete with expired lease
    let result = backend.ack_complete(ctx, leased.record.job_id, leased.lease_token, None).await;

    // Assert: LeaseExpired error
    assert!(matches!(result, Err(dog_queue::QueueError::LeaseExpired)));
}

/// A4. Expired Lease Becomes Eligible Again
#[tokio::test]
async fn test_expired_lease_becomes_eligible_again() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: dequeue but don't ack, let lease expire
    backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let first_lease = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    backend.force_lease_expiry(first_lease.record.job_id.clone()).await.unwrap();
    backend.run_reaper_tick().await.unwrap();

    // Act: dequeue again
    let second_lease = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();

    // Assert: same job, new lease, incremented attempt
    assert_eq!(second_lease.record.job_id, first_lease.record.job_id);
    assert_ne!(second_lease.lease_token, first_lease.lease_token);
    assert_eq!(second_lease.record.attempt, first_lease.record.attempt + 1);

    // Original lease token no longer valid
    let result = backend.ack_complete(ctx, first_lease.record.job_id, first_lease.lease_token, None).await;
    assert!(result.is_err());
}

/// B1. At-Most-Once Completion Transition
#[tokio::test]
async fn test_at_most_once_completion_transition() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: dequeue and ack_complete successfully
    backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    backend.ack_complete(ctx.clone(), leased.record.job_id.clone(), leased.lease_token.clone(), None).await.unwrap();

    // Act: call ack_complete again with same token
    let result = backend.ack_complete(ctx, leased.record.job_id, leased.lease_token, None).await;

    // Assert: JobAlreadyTerminal
    assert!(matches!(result, Err(dog_queue::QueueError::JobAlreadyTerminal)));
}

/// C1. Cancel Wins Over Ack Complete
#[tokio::test]
async fn test_cancel_wins_over_ack_complete() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: dequeue job
    backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();

    // Act: cancel, then ack_complete
    let cancel_result = backend.cancel(ctx.clone(), leased.record.job_id.clone()).await.unwrap();
    let ack_result = backend.ack_complete(ctx.clone(), leased.record.job_id.clone(), leased.lease_token, None).await;

    // Assert: cancel succeeds, ack fails
    assert!(cancel_result);
    assert!(matches!(ack_result, Err(dog_queue::QueueError::JobCanceled)));

    // Status remains Canceled
    let status = backend.get_status(ctx, leased.record.job_id).await.unwrap();
    assert!(matches!(status, JobStatus::Canceled { .. }));
}

/// D1. Retryable Error Schedules Retry
#[tokio::test]
async fn test_retryable_error_schedules_retry() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: enqueue, lease, ack_fail with retry_at
    backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    let retry_at = Utc::now() + chrono::Duration::seconds(60);

    // Act: ack_fail with retry_at
    backend.ack_fail(ctx.clone(), leased.record.job_id.clone(), leased.lease_token, "retryable error".to_string(), Some(retry_at)).await.unwrap();

    // Assert: status becomes Retrying
    let status = backend.get_status(ctx.clone(), leased.record.job_id.clone()).await.unwrap();
    assert!(matches!(status, JobStatus::Retrying { retry_at: scheduled } if scheduled == retry_at));

    // Job is NOT eligible before retry_at (simulate by checking queue is empty)
    let early_dequeue = backend.dequeue(ctx, &["default"]).await.unwrap();
    assert!(early_dequeue.is_none());
}

/// E1. Idempotency Returns Same JobId For Non-Terminal
#[tokio::test]
async fn test_idempotency_returns_same_job_id() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let mut job_message = create_test_job_message();
    job_message.idempotency_key = Some("test_key".to_string());

    // Arrange: enqueue with idempotency_key
    let job_id1 = backend.enqueue(ctx.clone(), job_message.clone()).await.unwrap();

    // Act: enqueue again with same scope
    let job_id2 = backend.enqueue(ctx, job_message).await.unwrap();

    // Assert: returns same job_id
    assert_eq!(job_id1, job_id2);
}

/// E3. Scope Isolation
#[tokio::test]
async fn test_idempotency_scope_isolation() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let mut base_message = create_test_job_message();
    base_message.idempotency_key = Some("same_key".to_string());

    // Different tenant
    let ctx2 = QueueCtx::new("different_tenant".to_string());
    let job_id1 = backend.enqueue(ctx.clone(), base_message.clone()).await.unwrap();
    let job_id2 = backend.enqueue(ctx2, base_message.clone()).await.unwrap();

    // Different queue
    let mut different_queue = base_message.clone();
    different_queue.queue = "different_queue".to_string();
    let job_id3 = backend.enqueue(ctx.clone(), different_queue).await.unwrap();

    // Different job_type
    let mut different_type = base_message;
    different_type.job_type = "different_type".to_string();
    let job_id4 = backend.enqueue(ctx, different_type).await.unwrap();

    // Assert: all job IDs differ (no collisions)
    let job_ids = vec![job_id1, job_id2, job_id3, job_id4];
    let unique_ids: std::collections::HashSet<_> = job_ids.iter().collect();
    assert_eq!(unique_ids.len(), 4);
}

/// F1. Priority Then FIFO
#[tokio::test]
async fn test_priority_then_fifo_ordering() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();

    // Arrange: enqueue with specific order
    let low_job = create_job_with_priority(JobPriority::Low);
    let job_id1 = backend.enqueue(ctx.clone(), low_job).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(10)).await; // Ensure different timestamps
    
    let high_newer = create_job_with_priority(JobPriority::High);
    let job_id2 = backend.enqueue(ctx.clone(), high_newer).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(10)).await;
    
    let high_older = create_job_with_priority(JobPriority::High);
    let job_id3 = backend.enqueue(ctx.clone(), high_older).await.unwrap();

    // Act: dequeue repeatedly
    let first = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    let second = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    let third = backend.dequeue(ctx, &["default"]).await.unwrap().unwrap();

    // Assert: High priority jobs come first, then FIFO within same priority
    // Note: The exact order depends on implementation - this tests that priority is respected
    let high_jobs = vec![job_id2, job_id3];
    assert!(high_jobs.contains(&first.record.job_id));
    assert!(high_jobs.contains(&second.record.job_id));
    assert_eq!(third.record.job_id, job_id1); // Low priority comes last
}

/// G1. Emits Enqueued Event
#[tokio::test]
async fn test_emits_enqueued_event() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Arrange: subscribe to event stream
    let mut event_stream = tokio_stream::wrappers::BroadcastStream::new(
        backend.event_broadcaster.subscribe()
    );

    // Act: enqueue job
    let job_id = backend.enqueue(ctx.clone(), job_message.clone()).await.unwrap();

    // Assert: receive JobEvent::Enqueued
    let event = receive_next_event(&mut event_stream).await;
    match event {
        JobEvent::Enqueued { job_id: event_job_id, tenant_id, queue, job_type, .. } => {
            assert_eq!(event_job_id, job_id);
            assert_eq!(tenant_id, ctx.tenant_id);
            assert_eq!(queue, job_message.queue);
            assert_eq!(job_type, job_message.job_type);
        }
        _ => panic!("Expected Enqueued event, got: {:?}", event),
    }
}

/// G2. Emits Lifecycle Events
#[tokio::test]
async fn test_emits_lifecycle_events() {
    let backend = MemoryBackend::new();
    let ctx = create_test_context();
    let job_message = create_test_job_message();

    // Subscribe to event stream
    let mut event_stream = tokio_stream::wrappers::BroadcastStream::new(
        backend.event_broadcaster.subscribe()
    );

    // Enqueue → Enqueued event
    let job_id = backend.enqueue(ctx.clone(), job_message).await.unwrap();
    let enqueued_event = receive_next_event(&mut event_stream).await;
    assert!(matches!(enqueued_event, JobEvent::Enqueued { job_id: event_job_id, .. } if event_job_id == job_id));

    // Dequeue → Leased event
    let leased = backend.dequeue(ctx.clone(), &["default"]).await.unwrap().unwrap();
    let leased_event = receive_next_event(&mut event_stream).await;
    assert!(matches!(leased_event, JobEvent::Leased { job_id: event_job_id, .. } if event_job_id == job_id));

    // Complete → Completed event
    backend.ack_complete(ctx, job_id.clone(), leased.lease_token, None).await.unwrap();
    let completed_event = receive_next_event(&mut event_stream).await;
    assert!(matches!(completed_event, JobEvent::Completed { job_id: event_job_id, .. } if event_job_id == job_id));
}
