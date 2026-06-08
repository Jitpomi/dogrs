//! # dog-queue: Production-Grade Job Processing Infrastructure
//!
//! **Multi-tenant job queue with superior correctness guarantees**
//!
//! dog-queue delivers genuine advantages over existing solutions like Apalis
//! through stronger semantics and multi-tenant design:
//!
//! ## 🎯 Production-Ready Features
//!
//! - **Stronger Correctness**: Lease tokens + expiry reaper + cancel-wins + tenant-scoped idempotency
//! - **Multi-Tenant by Design**: Tenant isolation built into API contract, not manual key prefixing
//! - **Reference Payloads**: Minimal serialization using BlobId/TrackId references for DogRS integration
//! - **Type-Safe Handlers**: Compile-time job definitions with runtime dispatch only at job-type boundary
//! - **Unified Semantics**: Consistent lease behavior across Memory, Redis, PostgreSQL backends
//! - **Structured Observability**: Event streams and distributed tracing, not just basic metrics
//! - **Safe Backend Migration**: Consistent semantics enable zero-downtime storage transitions
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use dog_queue::prelude::*;
//! use serde::{Deserialize, Serialize};
//!
//! // Define a job — payload must be Serialize + Deserialize
//! #[derive(Serialize, Deserialize)]
//! struct SendEmailJob {
//!     recipient: String,
//!     subject: String,
//! }
//!
//! // Shared execution context (e.g. database pool, SMTP client)
//! #[derive(Clone)]
//! struct AppContext {
//!     smtp_host: String,
//! }
//!
//! #[async_trait]
//! impl Job for SendEmailJob {
//!     type Context = AppContext;
//!     type Result = ();
//!
//!     const JOB_TYPE: &'static str = "send_email";
//!     const PRIORITY: JobPriority = JobPriority::Normal;
//!     const MAX_RETRIES: u32 = 3;
//!
//!     async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, JobError> {
//!         // use ctx.smtp_host to send self.recipient / self.subject
//!         Ok(())
//!     }
//! }
//!
//! // Wire up the adapter
//! let backend = dog_queue::backend::memory::MemoryBackend::new();
//! let adapter = QueueAdapter::new(backend);
//! adapter.register_job::<SendEmailJob>().await?;
//!
//! // Enqueue from any tenant context
//! let ctx = QueueCtx::new("tenant_abc".to_string());
//! let job_id = adapter.enqueue(ctx.clone(), SendEmailJob {
//!     recipient: "user@example.com".to_string(),
//!     subject: "Welcome!".to_string(),
//! }).await?;
//!
//! // Start a worker — it polls and dispatches jobs automatically
//! let app_ctx = AppContext { smtp_host: "smtp.example.com".to_string() };
//! let handle = adapter
//!     .start_workers(ctx, app_ctx, vec!["send_email".to_string()])
//!     .await?;
//!
//! // Graceful shutdown
//! handle.shutdown().await?;
//! ```

// Production-ready architecture modules
pub mod adapter;
pub mod backend;
pub mod codec;
pub mod error;
pub mod job;
pub mod observability;
pub mod types;

#[cfg(test)]
mod tests;

// Optional advanced features (placeholder for future implementation)
// #[cfg(feature = "workflows")]
// pub mod workflow;

// #[cfg(feature = "scheduling")]
// pub mod scheduling;

// Core API exports - standardize on QueueAdapter for DogRS consistency
pub use adapter::QueueAdapter;
pub use adapter::{QueueConfig, WorkerHandle};
pub use backend::QueueBackend;
pub use codec::json::JsonCodec;
pub use codec::{CodecRegistry, JobCodec};
pub use error::{JobError, QueueError, QueueResult};
pub use job::{Job, JobRegistry};
pub use types::{
    JobEvent, JobId, JobMessage, JobPriority, JobRecord, JobStatus, LeasedJob, QueueCapabilities,
    QueueCtx,
};

// Observability exports
pub use observability::{LiveMetrics, ObservabilityLayer};

// Optional feature exports
#[cfg(feature = "cron-scheduling")]
pub use scheduling::{Schedule, Scheduler};

// Backend implementations
#[cfg(feature = "redis")]
pub use backend::redis::RedisBackend;

#[cfg(feature = "postgres")]
pub use backend::postgres::PostgresBackend;

#[cfg(feature = "sqlite")]
pub use backend::sqlite::SqliteBackend;

// Observability features
#[cfg(feature = "metrics")]
pub use observability::metrics::{MetricsCollector, PrometheusExporter};

#[cfg(feature = "tracing-opentelemetry")]
pub use observability::tracing::{DistributedTracing, SpanCollector};

#[cfg(feature = "ui")]
pub use observability::ui::WebUI;

/// Production-ready prelude for multi-tenant job processing
pub mod prelude {
    // Core engine and types
    pub use crate::{Job, QueueAdapter, QueueBackend};

    // Essential types
    pub use crate::{JobError, JobId, JobPriority, JobStatus, QueueCtx, QueueResult};

    // Codec system
    pub use crate::{CodecRegistry, JobCodec, JsonCodec};

    // Job registry
    pub use crate::JobRegistry;

    // Observability
    pub use crate::{LiveMetrics, ObservabilityLayer};

    // Essential traits
    pub use async_trait::async_trait;

    // Optional features (placeholder for future implementation)
    // #[cfg(feature = "workflows")]
    // pub use crate::{Workflow, WorkflowBuilder};

    // #[cfg(feature = "scheduling")]
    // pub use crate::{Schedule, Scheduler};
}
