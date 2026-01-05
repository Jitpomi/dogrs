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
        lease_until: DateTime<Utc>,
        at: DateTime<Utc>,
    },
    
    /// Job is being retried
    Retrying {
        job_id: JobId,
        retry_at: DateTime<Utc>,
        error: String,
        at: DateTime<Utc>,
    },
    
    /// Job completed successfully
    Completed {
        job_id: JobId,
        at: DateTime<Utc>,
    },
    
    /// Job failed permanently
    Failed {
        job_id: JobId,
        error: String,
        at: DateTime<Utc>,
    },
    
    /// Job was canceled
    Canceled {
        job_id: JobId,
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
        }
    }

    /// Get the job ID from any event
    pub fn job_id(&self) -> &JobId {
        match self {
            Self::Enqueued { job_id, .. } => job_id,
            Self::Leased { job_id, .. } => job_id,
            Self::Retrying { job_id, .. } => job_id,
            Self::Completed { job_id, .. } => job_id,
            Self::Failed { job_id, .. } => job_id,
            Self::Canceled { job_id, .. } => job_id,
        }
    }

    /// Get the timestamp from any event
    pub fn timestamp(&self) -> &DateTime<Utc> {
        match self {
            Self::Enqueued { at, .. } => at,
            Self::Leased { at, .. } => at,
            Self::Retrying { at, .. } => at,
            Self::Completed { at, .. } => at,
            Self::Failed { at, .. } => at,
            Self::Canceled { at, .. } => at,
        }
    }
}
