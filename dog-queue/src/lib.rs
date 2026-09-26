//! Tenant-scoped job processing with typed handlers and lease-based ownership.
//!
//! Memory is process-local. PostgreSQL, Redis and JetStream provide persistent
//! ledgers. RabbitMQ, Kafka, SQS and Pub/Sub supply notifications alongside a ledger.
//! See the crate README for durability, migration, capacity and deployment requirements.

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

#[cfg(feature = "cron-scheduling")]
pub mod scheduling;

#[cfg(feature = "cron-scheduling")]
pub use scheduling::Schedule;
#[cfg(feature = "cron-scheduling")]
pub use scheduling::Scheduler;

pub use adapter::QueueAdapter;
pub use adapter::{QueueConfig, WorkerHandle};
#[cfg(feature = "postgres")]
pub use backend::postgres::PostgresBackend;
#[cfg(feature = "redis")]
pub use backend::redis::RedisBackend;
pub use backend::QueueBackend;
pub use codec::json::JsonCodec;
pub use codec::{CodecRegistry, EnqueueOptions, JobCodec};
pub use error::{JobError, QueueError, QueueResult};
pub use job::{Job, JobRegistry};
pub use types::{
    JobEvent, JobId, JobMessage, JobPriority, JobRecord, JobStatus, LeaseToken, LeasedJob,
    QueueCapabilities, QueueCtx, QueueFeature,
};

// Observability exports
pub use observability::{LiveMetrics, ObservabilityLayer, PerformanceAnalytics};

// Observability features
#[cfg(feature = "metrics")]
pub use observability::metrics::{MetricsCollector, PrometheusExporter};

/// Prelude for multi-tenant job processing
pub mod prelude {
    // Core engine and types
    pub use crate::{Job, QueueAdapter, QueueBackend};

    // Essential types
    pub use crate::{JobError, JobId, JobPriority, JobStatus, LeaseToken, QueueCtx, QueueResult};

    // Adapter configuration and lifecycle
    pub use crate::{EnqueueOptions, QueueConfig, WorkerHandle};

    // Codec system
    pub use crate::{CodecRegistry, JobCodec, JsonCodec};

    // Job registry
    pub use crate::JobRegistry;

    // Observability
    pub use crate::{LiveMetrics, ObservabilityLayer, PerformanceAnalytics};

    // Essential traits
    pub use async_trait::async_trait;

    // Optional features (placeholder for future implementation)
    // #[cfg(feature = "workflows")]
    // pub use crate::{Workflow, WorkflowBuilder};

    #[cfg(feature = "cron-scheduling")]
    pub use crate::{Schedule, Scheduler};
}
