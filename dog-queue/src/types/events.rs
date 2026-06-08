use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::JobId;

/// Minimal stable event protocol for structured observability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobEvent {
    /// Job was enqueued
    Enqueued {
        job_id: JobId,
        tenant_id: String,
        queue: String,
        job_type: String,
        at: DateTime<Utc>,
    },

    /// Job was leased by a worker
    Leased {
        job_id: JobId,
        tenant_id: String,
        lease_until: DateTime<Utc>,
        at: DateTime<Utc>,
    },

    /// Job is being retried
    Retrying {
        job_id: JobId,
        tenant_id: String,
        retry_at: DateTime<Utc>,
        error: String,
        at: DateTime<Utc>,
    },

    /// Job completed successfully
    Completed {
        job_id: JobId,
        tenant_id: String,
        at: DateTime<Utc>,
    },

    /// Job failed permanently
    Failed {
        job_id: JobId,
        tenant_id: String,
        error: String,
        at: DateTime<Utc>,
    },

    /// Job was canceled
    Canceled {
        job_id: JobId,
        tenant_id: String,
        at: DateTime<Utc>,
    },

    /// Job heartbeat extended — emitted each time `heartbeat_extend` succeeds.
    ///
    /// Event stream consumers (dashboards, liveness probes) can use this event
    /// to distinguish an actively-processing long-running job from a crashed or
    /// stalled worker. Without this variant the stream goes silent between the
    /// initial `Leased` event and the eventual `Completed`/`Failed` event.
    HeartbeatExtended {
        job_id: JobId,
        tenant_id: String,
        new_lease_until: DateTime<Utc>,
        at: DateTime<Utc>,
    },
}

impl JobEvent {
    /// Get event type name as string
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Enqueued { .. } => "enqueued",
            Self::Leased { .. } => "leased",
            Self::Retrying { .. } => "retrying",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::Canceled { .. } => "canceled",
            Self::HeartbeatExtended { .. } => "heartbeat_extended",
        }
    }

    /// Get the tenant ID from any event.
    ///
    /// Used by `event_stream` to filter events per tenant so that consumers
    /// only receive events from their own tenant's jobs.
    pub fn tenant_id(&self) -> &str {
        match self {
            Self::Enqueued { tenant_id, .. }
            | Self::Leased { tenant_id, .. }
            | Self::Retrying { tenant_id, .. }
            | Self::Completed { tenant_id, .. }
            | Self::Failed { tenant_id, .. }
            | Self::Canceled { tenant_id, .. }
            | Self::HeartbeatExtended { tenant_id, .. } => tenant_id,
        }
    }

    /// Get the job ID from any event
    pub fn job_id(&self) -> &JobId {
        match self {
            Self::Enqueued { job_id, .. }
            | Self::Leased { job_id, .. }
            | Self::Retrying { job_id, .. }
            | Self::Completed { job_id, .. }
            | Self::Failed { job_id, .. }
            | Self::Canceled { job_id, .. }
            | Self::HeartbeatExtended { job_id, .. } => job_id,
        }
    }

    /// Get the timestamp from any event
    pub fn timestamp(&self) -> &DateTime<Utc> {
        match self {
            Self::Enqueued { at, .. }
            | Self::Leased { at, .. }
            | Self::Retrying { at, .. }
            | Self::Completed { at, .. }
            | Self::Failed { at, .. }
            | Self::Canceled { at, .. }
            | Self::HeartbeatExtended { at, .. } => at,
        }
    }
}
