# Dog-Queue 🐕

A production-grade job queue library for Rust with multi-tenant semantics, lease-based processing, and type-safe job definitions.

[![Crates.io](https://img.shields.io/crates/v/dog-queue.svg)](https://crates.io/crates/dog-queue)
[![Documentation](https://docs.rs/dog-queue/badge.svg)](https://docs.rs/dog-queue)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

## Features

- 🏢 **Multi-tenant isolation** - Jobs scoped by tenant ID
- 🔒 **Lease-based processing** - Non-destructive job dequeue with acknowledgment
- 🎯 **Type-safe jobs** - Compile-time job type checking with const generics
- 🔄 **Automatic retries** - Configurable retry logic with exponential backoff
- 🚫 **Cancel-wins semantics** - Proper job cancellation handling
- 🔑 **Idempotency** - Duplicate job prevention with custom keys
- ⚡ **Priority queues** - Job prioritization support
- 🧹 **Lease expiry reaper** - Automatic recovery of expired leases
- 📊 **Built-in observability** - Metrics and tracing support
- 🔌 **Pluggable backends** - Memory, Redis, PostgreSQL support (planned)

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-queue = "0.1"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
```

## Example: Automated Email Management

Consider an email management service that processes users' inboxes automatically. The service needs to:

- Fetch new emails daily without duplicating work
- Analyze reading patterns to identify important messages  
- Generate AI summaries for key emails
- Archive or delete messages based on user preferences

This represents a common pattern: scheduled background jobs that must handle API failures, avoid duplicate processing, and maintain data consistency.

### 1. Define the Jobs

```rust
use dog_queue::{Job, JobError, JobPriority, QueueAdapter, QueueCtx};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

// Job 1: Daily inbox snapshot (scheduled, idempotent)
#[derive(Clone, Serialize, Deserialize)]
struct FetchInboxSnapshotJob {
    user_id: String,
    date: String, // "2024-01-15" - makes it idempotent per day
}

#[async_trait::async_trait]
impl Job for FetchInboxSnapshotJob {
    type Context = EmailService;
    type Result = InboxSnapshot;

    const JOB_TYPE: &'static str = "fetch_inbox_snapshot";
    const PRIORITY: JobPriority = JobPriority::High; // Users expect fresh data
    const MAX_RETRIES: u32 = 5; // Gmail API can be flaky

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let snapshot = ctx.fetch_inbox_metadata(&self.user_id, &self.date).await?;
        ctx.store_snapshot(&self.user_id, &self.date, &snapshot).await?;
        Ok(snapshot)
    }
}

// Job 2: Analyze reading patterns (failure-tolerant)
#[derive(Clone, Serialize, Deserialize)]
struct AnalyzeReadingPatternsJob {
    user_id: String,
    email_ids: Vec<String>,
}

#[async_trait::async_trait]
impl Job for AnalyzeReadingPatternsJob {
    type Context = EmailService;
    type Result = ReadingAnalysis;

    const JOB_TYPE: &'static str = "analyze_reading_patterns";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let analysis = ctx.analyze_user_patterns(&self.user_id, &self.email_ids).await?;
        Ok(analysis)
    }
}

// Job 3: Generate summaries (expensive, can be canceled)
#[derive(Clone, Serialize, Deserialize)]
struct GenerateSummariesJob {
    user_id: String,
    important_email_ids: Vec<String>,
}

#[async_trait::async_trait]
impl Job for GenerateSummariesJob {
    type Context = EmailService;
    type Result = Vec<EmailSummary>;

    const JOB_TYPE: &'static str = "generate_summaries";
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 2; // AI APIs are expensive

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let summaries = ctx.generate_ai_summaries(&self.user_id, &self.important_email_ids).await?;
        Ok(summaries)
    }
}

// Job 4: Bulk archive/delete (must survive crashes)
#[derive(Clone, Serialize, Deserialize)]
struct CleanupInboxJob {
    user_id: String,
    archive_ids: Vec<String>,
    delete_ids: Vec<String>,
}

#[async_trait::async_trait]
impl Job for CleanupInboxJob {
    type Context = EmailService;
    type Result = CleanupResult;

    const JOB_TYPE: &'static str = "cleanup_inbox";
    const PRIORITY: JobPriority = JobPriority::Low; // Cleanup happens in background
    const MAX_RETRIES: u32 = 5; // Must complete eventually

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        let result = ctx.bulk_cleanup(&self.user_id, &self.archive_ids, &self.delete_ids).await?;
        Ok(result)
    }
}

// Your email service context
#[derive(Clone)]
struct EmailService {
    // Gmail API client, AI service, database, etc.
}

// Result types
#[derive(Clone, Serialize, Deserialize)]
struct InboxSnapshot {
    total_emails: u32,
    new_emails: Vec<String>,
    timestamp: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ReadingAnalysis {
    never_read_senders: Vec<String>,
    important_keywords: Vec<String>,
    junk_patterns: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct EmailSummary {
    email_id: String,
    summary: String,
    importance_score: f32,
}

#[derive(Clone, Serialize, Deserialize)]
struct CleanupResult {
    archived_count: u32,
    deleted_count: u32,
    failed_ids: Vec<String>,
}
```

### 2. Schedule the Jobs

```rust
use dog_queue::{QueueAdapter, QueueCtx, JobMessage, backend::memory::MemoryBackend};
use chrono::{Utc, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = MemoryBackend::new();
    let adapter = QueueAdapter::new(backend);
    
    // Register job types
    adapter.register_job::<FetchInboxSnapshotJob>().await?;
    adapter.register_job::<AnalyzeReadingPatternsJob>().await?;
    adapter.register_job::<GenerateSummariesJob>().await?;
    adapter.register_job::<CleanupInboxJob>().await?;
    
    let user_ctx = QueueCtx::new("alice@example.com".to_string());
    let today = Utc::now().format("%Y-%m-%d").to_string();
    
    // Schedule daily inbox snapshot with idempotency
    let snapshot_job = FetchInboxSnapshotJob {
        user_id: "alice@example.com".to_string(),
        date: today.clone(),
    };
    
    let snapshot_message = JobMessage {
        job_type: "fetch_inbox_snapshot".to_string(),
        payload_bytes: serde_json::to_vec(&snapshot_job)?,
        codec: "json".to_string(),
        queue: "default".to_string(),
        priority: JobPriority::High,
        max_retries: 5,
        run_at: Utc::now(),
        idempotency_key: Some(format!("snapshot_{}_{}", "alice@example.com", today)),
    };
    
    let snapshot_job_id = adapter.backend().enqueue(user_ctx.clone(), snapshot_message).await?;
    
    // Schedule follow-up jobs with delays
    let analysis_job = AnalyzeReadingPatternsJob {
        user_id: "alice@example.com".to_string(),
        email_ids: vec!["email_1".to_string(), "email_2".to_string()],
    };
    
    let analysis_message = JobMessage {
        job_type: "analyze_reading_patterns".to_string(),
        payload_bytes: serde_json::to_vec(&analysis_job)?,
        codec: "json".to_string(),
        queue: "default".to_string(),
        priority: JobPriority::Normal,
        max_retries: 3,
        run_at: Utc::now() + Duration::minutes(5),
        idempotency_key: Some(format!("analysis_{}_{}", "alice@example.com", today)),
    };
    
    adapter.backend().enqueue(user_ctx.clone(), analysis_message).await?;
    
    println!("Scheduled email processing pipeline for {}", today);
    Ok(())
```rust
use dog_queue::{QueueAdapter, QueueCtx, backend::memory::MemoryBackend};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

async fn run_inbox_workers() -> Result<(), Box<dyn std::error::Error>> {
    let backend = MemoryBackend::new();
    let adapter = Arc::new(QueueAdapter::new(backend));
    
    // Register all inbox cleaning job types
    adapter.register_job::<FetchInboxSnapshotJob>().await?;
    adapter.register_job::<AnalyzeReadingPatternsJob>().await?;
    adapter.register_job::<GenerateSummariesJob>().await?;
    adapter.register_job::<CleanupInboxJob>().await?;
    
    let mut handles = vec![];
    
    // Start Alice's inbox worker pool (multi-tenant)
    let alice_ctx = QueueCtx::new("alice@example.com");
    let alice_service = EmailService::new().await;
    let alice_handle = adapter.start_workers(
        alice_ctx,
        alice_service,
        vec!["default".to_string()]
    ).await?;
    handles.push(alice_handle);
    
    // Start Bob's inbox worker pool (completely isolated from Alice)
    let bob_ctx = QueueCtx::new("bob@company.com");
    let bob_service = EmailService::new().await;
    let bob_handle = adapter.start_workers(
        bob_ctx,
        bob_service,
        vec!["default".to_string()]
    ).await?;
    handles.push(bob_handle);
    
    // Wait for all worker pools (they run until shutdown)
    for handle in handles {
        // You can also use handle.shutdown().await to stop gracefully
        // handle.await? is not directly awaitable in the same way for the JoinHandle wrapper,
        // but this shows the intent of keeping the process alive.
    }
    
    Ok(())
}

impl EmailService {
    async fn new() -> Self {
        Self {
            // Initialize Gmail API client, AI service, database, etc.
        }
    }
    
    async fn fetch_inbox_metadata(&self, user_id: &str, date: &str) -> Result<InboxSnapshot, JobError> {
        // Simulate Gmail API call (can fail due to rate limits, network issues)
        println!("� Fetching inbox metadata for {} on {}...", user_id, date);
        sleep(Duration::from_secs(2)).await; // Simulate API call
        
        // This is idempotent - same date always returns same snapshot
        Ok(InboxSnapshot {
            total_emails: 150,
            new_emails: vec!["email_1".to_string(), "email_2".to_string()],
            timestamp: chrono::Utc::now(),
        })
    }
    
    async fn analyze_user_patterns(&self, user_id: &str, email_ids: &[String]) -> Result<ReadingAnalysis, JobError> {
        // Simulate ML inference (can fail due to model errors, timeouts)
        println!("🧠 Analyzing reading patterns for {}...", user_id);
        sleep(Duration::from_secs(3)).await; // Simulate ML processing
        
        Ok(ReadingAnalysis {
            never_read_senders: vec!["newsletter@spam.com".to_string()],
            important_keywords: vec!["urgent".to_string(), "invoice".to_string()],
            junk_patterns: vec!["unsubscribe".to_string()],
        })
    }
    
    async fn generate_ai_summaries(&self, user_id: &str, email_ids: &[String]) -> Result<Vec<EmailSummary>, JobError> {
        // Simulate expensive AI API calls (can fail, user might cancel)
        println!("✨ Generating AI summaries for {} emails...", email_ids.len());
        sleep(Duration::from_secs(5)).await; // Simulate AI processing
        
        Ok(vec![
            EmailSummary {
                email_id: "important_1".to_string(),
                summary: "Meeting moved to 3 PM tomorrow".to_string(),
                importance_score: 0.9,
            },
            EmailSummary {
                email_id: "important_2".to_string(),
                summary: "Invoice #1234 due in 5 days".to_string(),
                importance_score: 0.8,
            },
        ])
    }
    
    async fn bulk_cleanup(&self, user_id: &str, archive_ids: &[String], delete_ids: &[String]) -> Result<CleanupResult, JobError> {
        // Simulate bulk Gmail operations (can partially fail, must survive crashes)
        println!("🧹 Cleaning up inbox: archiving {}, deleting {}...", archive_ids.len(), delete_ids.len());
        sleep(Duration::from_secs(4)).await; // Simulate bulk operations
        
        Ok(CleanupResult {
            archived_count: archive_ids.len() as u32,
            deleted_count: delete_ids.len() as u32,
            failed_ids: vec![], // In reality, some might fail
        })
    }
    
    async fn store_snapshot(&self, user_id: &str, date: &str, snapshot: &InboxSnapshot) -> Result<(), JobError> {
        // Store snapshot to prevent refetching
        println!("💾 Storing snapshot for {} on {}", user_id, date);
        Ok(())
    }
}
```

## Advanced Features

### Idempotency

Prevent duplicate job processing with idempotency keys:

```rust
use dog_queue::{JobMessage, JobPriority};

let message = JobMessage {
    job_type: "send_email".to_string(),
    payload_bytes: serde_json::to_vec(&job)?,
    codec: "json".to_string(),
    queue: "default".to_string(),
    priority: JobPriority::Normal,
    max_retries: 3,
    run_at: chrono::Utc::now(),
    idempotency_key: Some("user_123_welcome_email".to_string()),
};

let job_id = adapter.backend().enqueue(ctx, message).await?;
```

### Scheduled Jobs

Schedule jobs to run at a specific time:

```rust
let future_time = chrono::Utc::now() + chrono::Duration::hours(1);

let message = JobMessage {
    // ... other fields
    run_at: future_time,
    idempotency_key: None,
};
```

### Job Priorities

Control job processing order with priorities:

```rust
impl Job for UrgentEmailJob {
    // ... other implementations
    const PRIORITY: JobPriority = JobPriority::High;
}

impl Job for NewsletterJob {
    // ... other implementations  
    const PRIORITY: JobPriority = JobPriority::Low;
}
```

### Job Cancellation

Cancel jobs before they're processed:

```rust
// Cancel a specific job
adapter.backend().cancel_job(ctx, job_id).await?;

// Try to acknowledge completion of a canceled job
match adapter.backend().ack_complete(ctx, job_id, result).await {
    Err(QueueError::JobCanceled) => {
        println!("Job was canceled - cancel wins!");
    }
    Ok(_) => println!("Job completed"),
    Err(e) => println!("Other error: {}", e),
}
```

## Observability

Dog-queue includes built-in metrics and tracing:

```rust
// Access metrics
let metrics = adapter.observability().metrics();
let job_count = metrics.get_job_count("send_email");
let avg_duration = metrics.get_average_execution_time("send_email");

// Metrics are automatically collected for:
// - Job execution times
// - Success/failure rates  
// - Queue depths
// - Retry counts
```

## Backends

### Memory Backend (Development)

Perfect for development, testing, and single-node applications:

```rust
use dog_queue::backend::memory::MemoryBackend;

let backend = MemoryBackend::new();
```

### Redis Backend (Coming Soon)

For distributed, production deployments:

```rust
// Coming in v0.2
use dog_queue::backend::redis::RedisBackend;

let backend = RedisBackend::new("redis://localhost:6379").await?;
```

### PostgreSQL Backend (Coming Soon)

For applications already using PostgreSQL:

```rust
// Coming in v0.2
use dog_queue::backend::postgres::PostgresBackend;

let backend = PostgresBackend::new("postgresql://localhost/mydb").await?;
```

## Error Handling

Dog-queue provides comprehensive error types:

```rust
use dog_queue::{QueueError, JobError};

match adapter.enqueue(ctx, job).await {
    Ok(job_id) => println!("Job enqueued: {}", job_id),
    Err(QueueError::JobAlreadyExists) => println!("Job already exists (idempotency)"),
    Err(QueueError::InvalidJobType(job_type)) => println!("Unknown job type: {}", job_type),
    Err(QueueError::Internal(msg)) => println!("Internal error: {}", msg),
    Err(e) => println!("Other error: {}", e),
}
```

## Configuration

Customize queue behavior:

```rust
use dog_queue::{QueueConfig, QueueAdapter};
use std::time::Duration;

let config = QueueConfig {
    default_lease_duration: Duration::from_secs(300), // 5 minutes
    max_retry_backoff: Duration::from_secs(3600),     // 1 hour
    reaper_interval: Duration::from_secs(60),         // 1 minute
};

let adapter = QueueAdapter::with_config(backend, config);
```

## Testing

Dog-queue is designed to be easily testable:

```rust
#[tokio::test]
async fn test_job_processing() {
    let backend = MemoryBackend::new();
    let adapter = QueueAdapter::new(backend);
    
    adapter.register_job::<EmailJob>().await.unwrap();
    
    let ctx = QueueCtx::new("test_tenant".to_string());
    let job = EmailJob { /* ... */ };
    
    let job_id = adapter.enqueue(ctx.clone(), job).await.unwrap();
    
    // Verify job was enqueued
    let status = adapter.backend().get_status(ctx, job_id).await.unwrap();
    assert!(matches!(status, JobStatus::Enqueued));
}
```

## Examples

Check out the `examples/` directory for complete working examples:

- **Basic Usage** - Simple job enqueuing and processing
- **Multi-tenant** - Tenant isolation examples  
- **Scheduled Jobs** - Delayed job execution
- **Error Handling** - Comprehensive error handling patterns
- **Testing** - Unit testing strategies

## Roadmap

- ✅ Memory backend with full feature set
- ✅ Type-safe job definitions
- ✅ Multi-tenant isolation
- ✅ Lease-based processing
- ✅ Retry logic with backoff
- ✅ Job cancellation
- ✅ Idempotency support
- ✅ Basic observability
- 🔄 Redis backend (v0.2)
- 🔄 PostgreSQL backend (v0.2)
- 🔄 Full OpenTelemetry integration (v0.2)
- 🔄 Workflow engine (v0.3)
- 🔄 Cron scheduling (v0.3)
- 🔄 Web UI (v0.4)

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Comparison with Other Libraries

| Feature | Dog-Queue | Apalis | Faktory | Sidekiq |
|---------|-----------|--------|---------|---------|
| Language | Rust | Rust | Go/Multi | Ruby |
| Type Safety | ✅ Compile-time | ✅ Runtime | ❌ | ❌ |
| Multi-tenant | ✅ Built-in | ❌ | ❌ | ❌ |
| Lease Semantics | ✅ | ❌ | ✅ | ❌ |
| Cancel-wins | ✅ | ❌ | ❌ | ❌ |
| Idempotency | ✅ | ❌ | ❌ | ✅ |
| Memory Backend | ✅ | ❌ | ❌ | ❌ |

---

**Dog-Queue**: Because your jobs deserve better than fire-and-forget! 🐕‍🦺
