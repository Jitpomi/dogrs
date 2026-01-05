# dog-queue Backend Conformance Test Specification

**Black-box invariants that every backend (Memory, Redis, Postgres) must pass to claim "identical semantics"**

## Overview

This specification defines the precise behavioral contracts that all dog-queue backends must satisfy. These tests ensure that switching between Memory, Redis, and PostgreSQL backends requires no application code changes and maintains identical correctness guarantees.

## Test Categories

### A. Lease Semantics

#### A1. Dequeue Leases Atomically
```rust
// Arrange: enqueue one job
let job_id = backend.enqueue(ctx, job_message).await?;

// Act: dequeue
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();

// Assert: atomic lease assignment
assert_eq!(leased.record.job_id, job_id);
assert!(leased.lease_token.as_str().len() > 0);
assert!(leased.lease_until > Utc::now());

// Verify status reflects lease
let status = backend.get_status(ctx, job_id).await?;
assert!(matches!(status, JobStatus::Processing { lease_until }));

// Verify record shows lease details
let record = backend.get_record(ctx, job_id).await?;
assert_eq!(record.lease_token, Some(leased.lease_token.clone()));
assert_eq!(record.lease_until, Some(leased.lease_until));
```

#### A2. Only Lease Holder Can Ack
```rust
// Arrange: dequeue job (lease_token = T1)
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
let fake_token = LeaseToken::from("invalid_token");

// Act: ack_complete with different token
let result = backend.ack_complete(ctx, leased.record.job_id, fake_token, None).await;

// Assert: InvalidLeaseToken error
assert!(matches!(result, Err(QueueError::InvalidLeaseToken)));
```

#### A3. Lease Expiry Race → LeaseExpired
```rust
// Arrange: enqueue job, dequeue, force lease expiry
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
// Force lease expiry (test helper method)
backend.force_lease_expiry(leased.record.job_id).await?;

// Act: ack_complete with expired lease
let result = backend.ack_complete(ctx, leased.record.job_id, leased.lease_token, None).await;

// Assert: LeaseExpired error
assert!(matches!(result, Err(QueueError::LeaseExpired)));
```

#### A4. Expired Lease Becomes Eligible Again
```rust
// Arrange: dequeue but don't ack, let lease expire
let first_lease = backend.dequeue(ctx, &["default"]).await?.unwrap();
backend.force_lease_expiry(first_lease.record.job_id).await?;
backend.run_reaper_tick().await?; // Test helper

// Act: dequeue again
let second_lease = backend.dequeue(ctx, &["default"]).await?.unwrap();

// Assert: same job, new lease, incremented attempt
assert_eq!(second_lease.record.job_id, first_lease.record.job_id);
assert_ne!(second_lease.lease_token, first_lease.lease_token);
assert_eq!(second_lease.record.attempt, first_lease.record.attempt + 1);

// Original lease token no longer valid
let result = backend.ack_complete(ctx, first_lease.record.job_id, first_lease.lease_token, None).await;
assert!(result.is_err());
```

### B. At-Least-Once Execution / At-Most-Once Transition

#### B1. At-Most-Once Completion Transition
```rust
// Arrange: dequeue and ack_complete successfully
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
backend.ack_complete(ctx, leased.record.job_id, leased.lease_token.clone(), None).await?;

// Act: call ack_complete again with same token
let result = backend.ack_complete(ctx, leased.record.job_id, leased.lease_token, None).await;

// Assert: JobAlreadyTerminal
assert!(matches!(result, Err(QueueError::JobAlreadyTerminal)));
```

#### B2. Terminal States Immutable
```rust
// Arrange: make job Completed
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
backend.ack_complete(ctx, leased.record.job_id, leased.lease_token, None).await?;

// Act: attempt to cancel and ack_fail
let cancel_result = backend.cancel(ctx, leased.record.job_id).await;
let fail_result = backend.ack_fail(ctx, leased.record.job_id, LeaseToken::new(), "error".to_string(), None).await;

// Assert: both operations fail
assert!(matches!(cancel_result, Ok(false))); // Cancel returns false for terminal jobs
assert!(matches!(fail_result, Err(QueueError::JobAlreadyTerminal)));
```

### C. Cancel-Wins Semantics

#### C1. Cancel Wins Over Ack Complete
```rust
// Arrange: dequeue job
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();

// Act: cancel, then ack_complete
let cancel_result = backend.cancel(ctx, leased.record.job_id).await?;
let ack_result = backend.ack_complete(ctx, leased.record.job_id, leased.lease_token, None).await;

// Assert: cancel succeeds, ack fails
assert!(cancel_result);
assert!(matches!(ack_result, Err(QueueError::JobCanceled)));

// Status remains Canceled
let status = backend.get_status(ctx, leased.record.job_id).await?;
assert!(matches!(status, JobStatus::Canceled { .. }));
```

#### C2. Cancel Invalidates Lease
```rust
// Arrange: dequeue, cancel
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
backend.cancel(ctx, leased.record.job_id).await?;

// Act: attempt heartbeat_extend (if supported) or ack_fail
let heartbeat_result = backend.heartbeat_extend(ctx, leased.record.job_id, leased.lease_token.clone(), Duration::from_secs(30)).await;
let fail_result = backend.ack_fail(ctx, leased.record.job_id, leased.lease_token, "error".to_string(), None).await;

// Assert: both fail with JobCanceled
assert!(matches!(heartbeat_result, Err(QueueError::JobCanceled)));
assert!(matches!(fail_result, Err(QueueError::JobCanceled)));
```

### D. Retry Semantics

#### D1. Retryable Error Schedules Retry
```rust
// Arrange: enqueue, lease, ack_fail with retry_at
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
let retry_at = Utc::now() + Duration::from_secs(60);

// Act: ack_fail with retry_at
backend.ack_fail(ctx, leased.record.job_id, leased.lease_token, "retryable error".to_string(), Some(retry_at)).await?;

// Assert: status becomes Retrying
let status = backend.get_status(ctx, leased.record.job_id).await?;
assert!(matches!(status, JobStatus::Retrying { retry_at: scheduled } if scheduled == retry_at));

// Job is NOT eligible before retry_at
let early_dequeue = backend.dequeue(ctx, &["default"]).await?;
assert!(early_dequeue.is_none());

// Becomes eligible at/after retry_at (test helper to advance time)
backend.advance_time_to(retry_at + Duration::from_secs(1)).await?;
let retry_dequeue = backend.dequeue(ctx, &["default"]).await?;
assert!(retry_dequeue.is_some());
```

#### D2. Permanent Failure Is Terminal
```rust
// Arrange: lease job
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();

// Act: ack_fail with retry_at=None (permanent failure)
backend.ack_fail(ctx, leased.record.job_id, leased.lease_token, "permanent error".to_string(), None).await?;

// Assert: status Failed and immutable
let status = backend.get_status(ctx, leased.record.job_id).await?;
assert!(matches!(status, JobStatus::Failed { error, .. } if error == "permanent error"));

// Never becomes eligible again
let dequeue_result = backend.dequeue(ctx, &["default"]).await?;
assert!(dequeue_result.is_none());
```

#### D3. Max Retries Respected
```rust
// Arrange: job with max_retries=2
let mut job_message = create_test_job_message();
job_message.max_retries = 2;
let job_id = backend.enqueue(ctx, job_message).await?;

// Act: exhaust retries (attempt 1, 2, then 3 which exceeds max)
for attempt in 1..=3 {
    let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
    assert_eq!(leased.record.attempt, attempt);
    
    if attempt <= 2 {
        // Schedule retry
        let retry_at = Utc::now() + Duration::from_secs(1);
        backend.ack_fail(ctx, job_id, leased.lease_token, "retry error".to_string(), Some(retry_at)).await?;
        backend.advance_time_to(retry_at + Duration::from_secs(1)).await?;
    } else {
        // Final attempt - should transition to Failed
        backend.ack_fail(ctx, job_id, leased.lease_token, "final error".to_string(), Some(Utc::now())).await?;
    }
}

// Assert: transitions to Failed, never eligible again
let status = backend.get_status(ctx, job_id).await?;
assert!(matches!(status, JobStatus::Failed { .. }));

let dequeue_result = backend.dequeue(ctx, &["default"]).await?;
assert!(dequeue_result.is_none());
```

#### D4. Attempt Increments On Lease Acquisition
```rust
// Arrange: enqueue (attempt=0)
let job_id = backend.enqueue(ctx, job_message).await?;

// Act: dequeue
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();

// Assert: attempt == 1 immediately after lease
assert_eq!(leased.record.attempt, 1);
```

### E. Idempotency Semantics (Tenant-Scoped)

#### E1. Idempotency Returns Same JobId For Non-Terminal
```rust
// Arrange: enqueue with idempotency_key
let mut job_message = create_test_job_message();
job_message.idempotency_key = Some("test_key".to_string());
let job_id1 = backend.enqueue(ctx, job_message.clone()).await?;

// Act: enqueue again with same scope (tenant, queue, job_type, key)
let job_id2 = backend.enqueue(ctx, job_message).await?;

// Assert: returns same job_id
assert_eq!(job_id1, job_id2);
```

#### E2. Terminal Allows Re-Enqueue
```rust
// Arrange: complete a job with idempotency key
let mut job_message = create_test_job_message();
job_message.idempotency_key = Some("test_key".to_string());
let job_id1 = backend.enqueue(ctx, job_message.clone()).await?;

let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
backend.ack_complete(ctx, job_id1, leased.lease_token, None).await?;

// Act: enqueue again with same idempotency key
let job_id2 = backend.enqueue(ctx, job_message).await?;

// Assert: new job_id allowed (terminal jobs don't block re-enqueue)
assert_ne!(job_id1, job_id2);
```

#### E3. Scope Isolation
```rust
// Arrange: same key, different scopes
let base_message = create_test_job_message();
base_message.idempotency_key = Some("same_key".to_string());

// Different tenant
let ctx2 = QueueCtx::new("different_tenant".to_string());
let job_id1 = backend.enqueue(ctx, base_message.clone()).await?;
let job_id2 = backend.enqueue(ctx2, base_message.clone()).await?;

// Different queue
let mut different_queue = base_message.clone();
different_queue.queue = "different_queue".to_string();
let job_id3 = backend.enqueue(ctx, different_queue).await?;

// Different job_type
let mut different_type = base_message.clone();
different_type.job_type = "different_type".to_string();
let job_id4 = backend.enqueue(ctx, different_type).await?;

// Assert: all job IDs differ (no collisions)
let job_ids = vec![job_id1, job_id2, job_id3, job_id4];
let unique_ids: std::collections::HashSet<_> = job_ids.iter().collect();
assert_eq!(unique_ids.len(), 4);
```

### F. Ordering Semantics

#### F1. Priority Then FIFO
```rust
// Arrange: enqueue with specific order
let low_older = create_job_with_priority(JobPriority::Low);
let job_id1 = backend.enqueue(ctx, low_older).await?;
std::thread::sleep(Duration::from_millis(10)); // Ensure different timestamps

let high_newer = create_job_with_priority(JobPriority::High);
let job_id2 = backend.enqueue(ctx, high_newer).await?;
std::thread::sleep(Duration::from_millis(10));

let high_older = create_job_with_priority(JobPriority::High);
let job_id3 = backend.enqueue(ctx, high_older).await?;

// Act: dequeue repeatedly
let first = backend.dequeue(ctx, &["default"]).await?.unwrap();
let second = backend.dequeue(ctx, &["default"]).await?.unwrap();
let third = backend.dequeue(ctx, &["default"]).await?.unwrap();

// Assert: correct priority + FIFO order
// High(newer) should come before High(older) due to creation time
assert_eq!(first.record.job_id, job_id2);  // High(newer)
assert_eq!(second.record.job_id, job_id3); // High(older) 
assert_eq!(third.record.job_id, job_id1);  // Low(older)
```

### G. Observability Events (Minimal Stable Protocol)

#### G1. Emits Enqueued Event
```rust
// Arrange: subscribe to event stream
let mut event_stream = backend.event_stream(ctx);

// Act: enqueue job
let job_id = backend.enqueue(ctx, job_message).await?;

// Assert: receive JobEvent::Enqueued
let event = tokio::time::timeout(Duration::from_secs(1), event_stream.next()).await??;
match event {
    JobEvent::Enqueued { job_id: event_job_id, tenant_id, queue, job_type, .. } => {
        assert_eq!(event_job_id, job_id);
        assert_eq!(tenant_id, ctx.tenant_id);
        assert_eq!(queue, "default");
        assert_eq!(job_type, "test_job");
    }
    _ => panic!("Expected Enqueued event"),
}
```

#### G2. Emits Lifecycle Events
```rust
// Test each transition emits corresponding event
let mut event_stream = backend.event_stream(ctx);

// Enqueue → Enqueued event (tested above)
let job_id = backend.enqueue(ctx, job_message).await?;

// Dequeue → Leased event
let leased = backend.dequeue(ctx, &["default"]).await?.unwrap();
let leased_event = receive_next_event(&mut event_stream).await;
assert!(matches!(leased_event, JobEvent::Leased { job_id: event_job_id, .. } if event_job_id == job_id));

// Complete → Completed event
backend.ack_complete(ctx, job_id, leased.lease_token, None).await?;
let completed_event = receive_next_event(&mut event_stream).await;
assert!(matches!(completed_event, JobEvent::Completed { job_id: event_job_id, .. } if event_job_id == job_id));

// Test Failed, Retrying, Canceled events similarly...
```

## Backend Test Helpers

Each backend must provide test helpers for deterministic testing:

```rust
pub trait BackendTestHelpers {
    /// Force a lease to expire (for testing lease expiry)
    async fn force_lease_expiry(&self, job_id: JobId) -> QueueResult<()>;
    
    /// Run one reaper tick (for testing lease reclamation)
    async fn run_reaper_tick(&self) -> QueueResult<()>;
    
    /// Advance backend's concept of time (for testing retry_at)
    async fn advance_time_to(&self, target_time: DateTime<Utc>) -> QueueResult<()>;
}
```

## Conformance Requirements

1. **All backends must pass 100% of these tests** before claiming "identical semantics"
2. **Memory backend must pass before Redis work starts** - this validates the test suite
3. **Test suite must be backend-agnostic** - same test code runs against all backends
4. **Event ordering must be deterministic** - events arrive in chronological order
5. **Tenant isolation must be absolute** - no cross-tenant data leakage in any test

## Implementation Notes

- Use `#[tokio::test]` for async test functions
- Provide factory functions for creating test data (`create_test_job_message()`, etc.)
- Use `tokio::time::timeout` for event stream assertions to prevent hanging tests
- Each test should clean up its data or use isolated test contexts
- Consider property-based testing with `proptest` for edge cases
