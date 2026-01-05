# dog-queue Architecture

**Next-generation job processing engine that surpasses all existing solutions**

## Vision: Production-Grade Queue Infrastructure

dog-queue delivers **genuine advantages** over existing solutions like Apalis through superior semantics and multi-tenant design:

- **Stronger correctness guarantees** - Lease tokens + expiry reaper + cancel-wins + tenant-scoped idempotency as core invariants
- **Multi-tenant by design** - Tenant isolation built into the API contract, not "prefix keys yourself"
- **Reference payloads** - Zero-copy execution on memory backend; minimal serialization on durable backends using BlobId/TrackId references
- **Compile-time job safety** - Type-safe handlers with compile-time job definitions; runtime dispatch only at job-type boundary
- **Unified semantics** - Consistent lease behavior across Memory, Redis, PostgreSQL backends with capability detection
- **Codec registry + envelope discipline** - Safe payload evolution and cross-backend migration
- **Structured observability** - Event streams and distributed tracing, not just basic metrics

## Revolutionary Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Service Layer                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Typed Jobs  │  │ Workflows   │  │ Event Streams       │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                   Execution Engine                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │Zero-Copy Exec│ │Adaptive Pool│  │ Backpressure Ctrl  │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                 Observability Layer                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │Live Metrics │  │ Distributed │  │ Performance Analytics│ │
│  │             │  │ Tracing     │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                  Storage Abstraction                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │   Memory    │  │    Redis    │  │  PostgreSQL/SQLite  │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Revolutionary Advantages Over Apalis

### 🔒 **Stronger Correctness Guarantees**
dog-queue is explicitly built around lease tokens + cancel-wins + tenant-scoped idempotency as core invariants. Jobs cannot be lost or double-processed, even with worker failures.

### 🏢 **Multi-Tenant by Design**
Tenant isolation is built into the API contract as a first-class primitive, not manual key prefixing. This enables safe multi-tenant deployments without cross-tenant data leaks.

### 📦 **Reference Payloads for DogRS**
Optimized for DogRS patterns: BlobId/TrackId references with heavy data in dog-blob storage. Constant-size serialization regardless of audio file size.

### 🎯 **Type-Safe Job Handlers**
Compile-time job definitions with type-safe handlers. Runtime dispatch only occurs at the job-type boundary (unavoidable for heterogeneous queues).

### 🔄 **Unified Semantics Across Backends**
Identical lease semantics across Memory, Redis, PostgreSQL backends with explicit capability detection. Consistent behavior enables safe backend migration.

### 📊 **Structured Observability**
Event streams + distributed tracing + structured logging provide full visibility into job flow, tenant activity, and system health.

## Next-Generation API Design

### Zero-Copy Job Execution

```rust
use chrono::{DateTime, Utc};
use futures_core::Stream;
use std::pin::Pin;

// Production-ready Job trait with compile-time safety
#[async_trait::async_trait]
pub trait Job: Send + Sync + 'static {
    type Context: Send + Sync + Clone + 'static;
    type Result: Send + Sync + 'static;
    type Error: Into<JobError> + Send + Sync + 'static;

    // Compile-time job identification
    const JOB_TYPE: &'static str;
    const PRIORITY: JobPriority = JobPriority::Normal;
    const MAX_RETRIES: u32 = 3;

    // Type-safe execution with reference payloads
    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, Self::Error>;

    // Optional configuration
    fn timeout(&self) -> Option<std::time::Duration> { None }
    fn idempotency_key(&self) -> Option<String> { None }
}

// Example: Reference payload for audio processing
#[derive(Serialize, Deserialize)]
pub struct GenerateWaveformJob {
    pub track_id: TrackId,        // Reference, not audio data
    pub blob_id: BlobId,          // Reference to dog-blob storage
    pub resolution: u32,          // Small config data
}

// Constant-size serialization regardless of audio file size
```

### Unified Storage Abstraction

```rust
use chrono::{DateTime, Utc};
use futures_core::Stream;
use std::pin::Pin;

// Type alias for boxed streams (stable Rust compatible)
pub type BoxStream<T> = Pin<Box<dyn Stream<Item = T> + Send + 'static>>;

// Unified semantics across all storage backends
#[async_trait::async_trait]
pub trait QueueBackend: Send + Sync {
    // Core lease-based operations (identical semantics across backends)
    async fn enqueue(&self, ctx: QueueCtx, message: JobMessage) -> QueueResult<JobId>;
    async fn dequeue(&self, ctx: QueueCtx, queues: &[&str]) -> QueueResult<Option<LeasedJob>>;
    
    // Lease management (consistent across Memory/Redis/Postgres)
    async fn ack_complete(
        &self, ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, 
        result_ref: Option<String>
    ) -> QueueResult<()>;
    
    async fn ack_fail(
        &self, ctx: QueueCtx, job_id: JobId, lease_token: LeaseToken, 
        error: String, retry_at: Option<DateTime<Utc>>
    ) -> QueueResult<()>;
    
    // Multi-tenant operations
    async fn cancel(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<bool>;
    async fn get_status(&self, ctx: QueueCtx, job_id: JobId) -> QueueResult<JobStatus>;
    
    // Observability (boxed for stable Rust)
    fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent>;
    
    // Explicit capability detection
    fn capabilities(&self) -> QueueCapabilities;
}
```

### Revolutionary Queue Engine

```rust
// Production-grade queue engine with multi-tenant semantics
pub struct QueueEngine<B: QueueBackend> {
    backend: B,
    codec_registry: CodecRegistry,
    job_registry: JobRegistry,
    observability: ObservabilityLayer,
}

impl<B: QueueBackend> QueueEngine<B> {
    // Enqueue job for processing (proper queue semantics)
    pub async fn enqueue<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<JobId> {
        // Serialize only references (BlobId, TrackId), not heavy data
        let message = self.codec_registry.encode_job(&job, &ctx)?;
        self.backend.enqueue(ctx, message).await
    }
    
    // Execute job immediately (for tests/dev - bypasses durable storage)
    pub async fn execute_now<J: Job>(&self, ctx: QueueCtx, job: J) -> QueueResult<J::Result> {
        // Direct execution for testing/development
        let execution_context = self.create_execution_context::<J>(ctx);
        job.execute(execution_context).await
            .map_err(|e| QueueError::Internal(e.into().to_string()))
    }
    
    // Multi-tenant worker management
    pub async fn start_workers<C>(&self, ctx: QueueCtx, context: C, queues: Vec<String>) -> QueueResult<WorkerHandle> 
    where C: Clone + Send + Sync + 'static {
        // Workers respect tenant boundaries and lease semantics
        todo!("Worker implementation")
    }
    
    // Structured observability
    pub fn event_stream(&self, ctx: QueueCtx) -> BoxStream<JobEvent> {
        self.backend.event_stream(ctx)
    }
}
```

## Error Handling

### QueueError (Infrastructure)
- `JobNotFound`
- `InvalidLeaseToken`
- `JobCanceled`
- `CodecNotFound(codec_id)`
- `PayloadTooLarge { size, max }`
- `BackendUnsupported(feature)`
- `Internal(String)`

### JobError (Execution Outcome)
```rust
pub enum JobError {
    Retryable(String),    // Schedule retry if attempts remain
    Permanent(String),    // Fail immediately, no retry
}
```

## Data Structures

### Job Separation
```rust
// JobMessage (immutable submission data)
pub struct JobMessage {
    pub job_type: String,
    pub payload_bytes: Vec<u8>,
    pub codec: String,
    pub queue: String,
    pub priority: JobPriority,
    pub max_retries: u32,
    pub run_at: DateTime<Utc>,
    pub idempotency_key: Option<String>,
}

// JobRecord (mutable runtime state)
pub struct JobRecord {
    pub job_id: JobId,
    pub tenant_id: String,
    pub message: JobMessage,
    pub status: JobStatus,
    pub attempt: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub lease_token: Option<LeaseToken>,
    pub lease_until: Option<DateTime<Utc>>,
}
```

### Job Status Lifecycle
```rust
pub enum JobStatus {
    Enqueued,
    Scheduled,
    Processing { lease_until: DateTime<Utc> },
    Retrying { retry_at: DateTime<Utc> },
    Completed { completed_at: DateTime<Utc> },
    Failed { failed_at: DateTime<Utc>, error: String },
    Canceled { canceled_at: DateTime<Utc> },
}
```

## Configuration

```rust
pub struct QueueConfig {
    pub default_queue: String,           // "default"
    pub worker_concurrency: usize,       // 4
    pub lease_duration: Duration,        // 60s
    pub poll_interval: Duration,         // 1s
    pub base_retry_delay: Duration,      // 5s
    pub max_retry_delay: Duration,       // 5m
    pub reaper_interval: Duration,       // 10s
    pub max_payload_bytes: usize,        // 256KB
}
```

## Memory Backend Implementation

### Core Storage
```rust
struct MemoryStore {
    jobs: HashMap<JobId, JobRecord>,
    idempotency: HashMap<ScopedIdemKey, JobId>,
    queue_index: HashMap<(String, String), Vec<JobId>>, // (tenant_id, queue) -> jobs
}
```

### Lease Expiry Reaper
- Periodic sweep (reaper_interval)
- Find `Processing` jobs with `lease_until < now`
- Transition to `Retrying` (if attempts remain) or `Failed`
- Respect cancel-wins: don't resurrect canceled jobs

### Worker Loop
1. `dequeue()` eligible jobs (run_at <= now, not terminal status)
2. Decode payload using `record.message.codec`
3. Dispatch by `job_type` to registered handler
4. Execute with tracing span
5. On success: `ack_complete()` (result goes to external storage)
6. On failure: compute `retry_at` with backoff, `ack_fail(retry_at)`
7. Repeat until graceful shutdown

### Priority + FIFO Ordering
```rust
// Correct FIFO ordering (older jobs first)
jobs.sort_by_key(|r| (Reverse(r.message.priority), r.created_at))
```

## Integration Example (Music-Blobs)

```rust
// Job payload (tenant-agnostic)
#[derive(Serialize, Deserialize)]
pub struct GenerateWaveformJob {
    pub track_id: String,
    pub audio_blob_id: String,  // Reference to dog-blob
    pub resolution: u32,
}

#[async_trait]
impl Job for GenerateWaveformJob {
    type Context = MusicServiceContext;
    type Result = ();

    async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
        // Load audio from dog-blob using ctx.tenant_id
        let blob_ctx = BlobCtx::new(ctx.tenant_id.clone());
        let audio_stream = ctx.blobs.open(blob_ctx, BlobId(self.audio_blob_id.clone()), None).await
            .map_err(|e| JobError::Retryable(format!("Failed to load audio: {}", e)))?;

        // Generate waveform peaks
        let peaks = generate_waveform_peaks(audio_stream, self.resolution).await
            .map_err(|e| JobError::Retryable(format!("Waveform generation failed: {}", e)))?;

        // Store result in blob metadata (not queue)
        let metadata = json!({ "waveform_peaks": peaks });
        ctx.blobs.update_metadata(blob_ctx, BlobId(self.audio_blob_id.clone()), metadata).await
            .map_err(|e| JobError::Permanent(format!("Failed to store metadata: {}", e)))?;

        Ok(())
    }

    fn job_type(&self) -> &'static str { "generate_waveform" }
    fn priority(&self) -> JobPriority { JobPriority::High }
    fn max_retries(&self) -> u32 { 2 }
}

// Service integration
impl MusicService {
    pub async fn upload_track(&self, ctx: TenantContext, audio_data: ByteStream) -> Result<TrackId> {
        // Store audio blob
        let receipt = self.blobs.put(ctx.clone(), put_request, audio_data).await?;

        // Queue background waveform generation with idempotency
        let job = GenerateWaveformJob {
            track_id: track_id.clone(),
            audio_blob_id: receipt.id.to_string(),
            resolution: 2000,
        };

        let queue_ctx = QueueCtx::new(ctx.tenant_id.clone())
            .with_trace_id(ctx.trace_id.clone());

        let options = EnqueueOptions {
            idempotency_key: Some(format!("waveform:{}", track_id)),
            ..Default::default()
        };

        self.queue.enqueue_with_options(queue_ctx, job, options).await?;
        Ok(track_id)
    }
}
```

## Credible Positioning Statement

**dog-queue is a DogRS-native queue foundation focused on strict distributed semantics (lease tokens, cancel-wins, tenant-scoped idempotency), reference payload patterns (BlobId/TrackId), and consistent behavior across backends via explicit capabilities — with workflows and UI layered on top.**

## Implementation Order

1. **types/*** - Core types (JobId, QueueCtx, JobStatus, JobMessage/JobRecord)
2. **error.rs** - QueueError and JobError enums
3. **codec/json.rs** - JSON codec + registry
4. **job/registry.rs** - Runtime job registry
5. **backend/mod.rs** - QueueBackend trait
6. **backend/memory/*** - Memory backend implementation
7. **adapter/worker.rs** - Worker loop with tracing and backoff
8. **backend/memory/reaper.rs** - Lease expiry sweep

## Implementation Roadmap

### Phase 1: Core Semantics (v0.1)
1. **Memory backend** - Reference implementation with full lease semantics
2. **Multi-tenant context** - QueueCtx with tenant isolation
3. **Codec registry** - Pluggable serialization with envelope discipline
4. **Type-safe job registry** - Compile-time job definitions

### Phase 2: Production Backend (v0.2)
1. **Redis backend** - Identical semantics to Memory using Redis streams + Lua scripts
2. **Lease expiry reaper** - Background process for dead worker recovery
3. **Event streaming** - Structured observability with distributed tracing
4. **Reference payload optimization** - BlobId/TrackId serialization patterns

### Phase 3: Advanced Features (v0.3)
1. **PostgreSQL backend** - Leveraging LISTEN/NOTIFY for real-time updates
2. **dog-queue-workflows** - Separate crate for DAG composition
3. **Monitoring UI** - Production-grade observability dashboard
4. **Cross-backend migration** - Safe transitions between storage systems

This roadmap delivers genuine advantages over Apalis through superior correctness guarantees and multi-tenant design, without overpromising on physically impossible features.

## Semantics Contract

**Precise distributed behavior guarantees that make dog-queue reliable**

### Lease Semantics
- **Lease acquisition**: `dequeue()` atomically assigns a lease token and expiry time
- **Lease validity**: Only the holder of a valid lease token can acknowledge the job
- **Lease expiry**: Jobs with expired leases become eligible for re-dequeue
- **Lease extension**: Optional capability to extend lease duration via heartbeat
- **Lease expiry race**: A worker may still finish after lease expiry; its ack will fail with `LeaseExpired` and the job may already be re-leased

### Exactly-Once vs At-Least-Once
- **Enqueue**: At-least-once; idempotency provides de-duplication per (tenant, queue, job_type, key)
- **Execution**: At-least-once (jobs may run more than once; users must be idempotent)
- **Acknowledgment**: At-most-once state transition (only one ack can win per lease token)
- **Result storage**: External to queue (dog-blob metadata, not queue state)

### Cancel-Wins Behavior
- **Priority**: `cancel()` operations take precedence over `ack_complete()`/`ack_fail()`
- **Race conditions**: If job is canceled while processing, ack operations return `JobCanceled` error
- **Terminal state**: Canceled jobs cannot transition to any other state
- **Lease invalidation**: Canceling a job immediately invalidates its lease

### Idempotency Scope and Rules
- **Scope**: Keys are scoped by `(tenant_id, queue, job_type, key)`
- **Collision handling**: Duplicate enqueue returns existing `JobId` if job not terminal
- **Key expiry**: Idempotency keys persist until job reaches terminal state
- **Cross-tenant isolation**: Keys cannot collide across different tenants

### Terminal Status Rules
- **Terminal states**: `Completed`, `Failed`, `Canceled`
- **Immutability**: Jobs in terminal states cannot change status
- **Lease cleanup**: Terminal jobs have their leases immediately cleared
- **Retry exhaustion**: Jobs exceeding `max_retries` transition to `Failed`

### Retry Rules for Error Types
- **Retryable errors**: Schedule retry with exponential backoff if attempts remain
- **Permanent errors**: Immediately transition to `Failed` status
- **Retry scheduling**: `retry_at` computed by adapter, not backend
- **Attempt tracking**: Increment attempt counter on lease acquisition (Enqueued/Retrying → Processing)

### Multi-Tenant Isolation
- **Context requirement**: All operations require valid `QueueCtx` with `tenant_id`
- **Data isolation**: Jobs from different tenants are never visible to each other
- **Backend scoping**: Backends MUST ensure queries/filtering are scoped by `ctx.tenant_id` for every operation
- **Queue names**: Not rewritten; scoping enforced by indexes/keys, not string construction
- **Capability inheritance**: Tenant context determines available backend capabilities

### Backend Consistency Requirements
- **Atomic dequeue**: Lease assignment must be atomic with job state change
- **Consistent ordering**: FIFO within priority levels: `(Reverse(priority), created_at)`
- **Durable leases**: Lease state must survive backend restarts
- **Event ordering**: Job events must be emitted in chronological order
