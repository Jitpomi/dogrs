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
//! ## 🚀 Reference Payload Quick Start
//! 
//! ```rust
//! use dog_queue::prelude::*;
//! use serde::{Deserialize, Serialize};
//! 
//! // Reference payload - constant size serialization
//! #[derive(Serialize, Deserialize)]
//! struct GenerateWaveformJob {
//!     track_id: TrackId,    // Reference, not audio data
//!     blob_id: BlobId,      // Reference to dog-blob storage
//!     resolution: u32,      // Small config data
//! }
//! 
//! #[async_trait::async_trait]
//! impl Job for GenerateWaveformJob {
//!     type Context = AudioContext;
//!     type Result = ();
//!     type Error = AudioError;
//!     
//!     // Type-safe execution with reference payloads
//!     async fn execute(&self, ctx: Self::Context) -> Result<Self::Result, Self::Error> {
//!         // Load audio from dog-blob using reference
//!         let audio_data = ctx.blob_store.get(&self.blob_id).await?;
//!         let waveform = generate_waveform(audio_data, self.resolution).await?;
//!         
//!         // Store result in blob metadata, not queue
//!         ctx.blob_store.update_metadata(&self.blob_id, waveform).await?;
//!         Ok(())
//!     }
//!     
//!     // Compile-time job identification
//!     const JOB_TYPE: &'static str = "generate_waveform";
//!     const PRIORITY: JobPriority = JobPriority::High;
//! }
//! 
//! // Multi-tenant queue with lease semantics
//! let engine = QueueEngine::new(redis_backend)
//!     .with_codec_registry()
//!     .with_observability();
//!     
//! let tenant_ctx = QueueCtx::new("tenant_123".to_string())
//!     .with_trace_id(trace_id);
//!     
//! let job_id = engine.enqueue(tenant_ctx, job).await?;
//! ```

// Production-ready architecture modules
pub mod engine;
pub mod types;
pub mod error;
pub mod codec;
pub mod job;
pub mod backend;
pub mod adapter;
pub mod observability;

// Optional advanced features (placeholder for future implementation)
// #[cfg(feature = "workflows")]
// pub mod workflow;

// #[cfg(feature = "scheduling")]
// pub mod scheduling;

// Core API exports - standardize on QueueAdapter for DogRS consistency
pub use adapter::QueueAdapter;
pub use types::{
    JobId, QueueCtx, JobPriority, JobStatus, JobMessage, JobRecord, 
    LeasedJob, QueueCapabilities, JobEvent
};
pub use error::{QueueError, QueueResult, JobError};
pub use codec::{JobCodec, CodecRegistry};
pub use codec::json::JsonCodec;
pub use job::{Job, JobRegistry};
pub use backend::QueueBackend;
pub use adapter::{WorkerHandle, QueueConfig};

// Observability exports
pub use observability::{ObservabilityLayer, LiveMetrics};

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
pub use observability::metrics::{PrometheusExporter, MetricsCollector};

#[cfg(feature = "tracing-opentelemetry")]
pub use observability::tracing::{DistributedTracing, SpanCollector};

#[cfg(feature = "ui")]
pub use observability::ui::WebUI;

/// Production-ready prelude for multi-tenant job processing
pub mod prelude {
    // Core engine and types
    pub use crate::{
        QueueAdapter, Job, QueueBackend
    };
    
    // Essential types
    pub use crate::{
        QueueCtx, JobId, JobPriority, JobStatus, JobError, QueueResult
    };
    
    // Codec system
    pub use crate::{
        JobCodec, JsonCodec, CodecRegistry
    };
    
    // Job registry
    pub use crate::JobRegistry;
    
    // Observability
    pub use crate::{ObservabilityLayer, LiveMetrics};
    
    // Essential traits
    pub use async_trait::async_trait;
    
    // Optional features (placeholder for future implementation)
    // #[cfg(feature = "workflows")]
    // pub use crate::{Workflow, WorkflowBuilder};
    
    // #[cfg(feature = "scheduling")]
    // pub use crate::{Schedule, Scheduler};
}
