use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{JobId, LeaseToken, JobMessage};

/// Job status lifecycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is queued and waiting to be processed
    Enqueued,
    
    /// Job is scheduled for future execution
    Scheduled,
    
    /// Job is currently being processed by a worker
    Processing { lease_until: DateTime<Utc> },
    
    /// Job failed and is waiting to be retried
    Retrying { retry_at: DateTime<Utc> },
    
    /// Job completed successfully
    Completed { completed_at: DateTime<Utc> },
    
    /// Job failed permanently (max retries exceeded or permanent error)
    Failed { failed_at: DateTime<Utc>, error: String },
    
    /// Job was canceled
    Canceled { canceled_at: DateTime<Utc> },
}

impl JobStatus {
    /// Check if the job is in a terminal state (completed, failed, or canceled)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed { .. } | Self::Canceled { .. }
        )
    }

    /// Check if the job is currently being processed
    pub fn is_processing(&self) -> bool {
        matches!(self, Self::Processing { .. })
    }

    /// Check if the job is eligible for processing (enqueued or retrying with retry_at <= now)
    pub fn is_eligible(&self, now: DateTime<Utc>) -> bool {
        match self {
            Self::Enqueued => true,
            Self::Retrying { retry_at } => *retry_at <= now,
            _ => false,
        }
    }

    /// Get the status name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
            Self::Scheduled => "scheduled",
            Self::Processing { .. } => "processing",
            Self::Retrying { .. } => "retrying",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::Canceled { .. } => "canceled",
        }
    }
}

/// Job record - mutable runtime state stored by backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    /// Unique job identifier
    pub job_id: JobId,
    
    /// Tenant identifier for isolation
    pub tenant_id: String,
    
    /// Immutable job message data
    pub message: JobMessage,
    
    /// Current job status
    pub status: JobStatus,
    
    /// Current attempt number (starts at 0)
    pub attempt: u32,
    
    /// When the job was created
    pub created_at: DateTime<Utc>,
    
    /// When the job was last updated
    pub updated_at: DateTime<Utc>,
    
    /// Last error message (if any)
    pub last_error: Option<String>,
    
    /// Current lease token (if processing)
    pub lease_token: Option<LeaseToken>,
    
    /// When the current lease expires (if processing)
    pub lease_until: Option<DateTime<Utc>>,
}

impl JobRecord {
    /// Create a new job record
    pub fn new(job_id: JobId, tenant_id: String, message: JobMessage) -> Self {
        let now = Utc::now();
        let status = if message.run_at > now {
            JobStatus::Scheduled
        } else {
            JobStatus::Enqueued
        };

        Self {
            job_id,
            tenant_id,
            message,
            status,
            attempt: 0,
            created_at: now,
            updated_at: now,
            last_error: None,
            lease_token: None,
            lease_until: None,
        }
    }

    /// Check if the job can be retried
    pub fn can_retry(&self) -> bool {
        self.attempt < self.message.max_retries && !self.status.is_terminal()
    }

    /// Check if the lease has expired
    pub fn lease_expired(&self, now: DateTime<Utc>) -> bool {
        match (&self.status, &self.lease_until) {
            (JobStatus::Processing { .. }, Some(lease_until)) => *lease_until < now,
            _ => false,
        }
    }

    /// Update the job status and timestamp
    pub fn update_status(&mut self, status: JobStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Set an error and update timestamp
    pub fn set_error(&mut self, error: String) {
        self.last_error = Some(error);
        self.updated_at = Utc::now();
    }

    /// Start processing with a lease
    pub fn start_processing(&mut self, lease_token: LeaseToken, lease_until: DateTime<Utc>) {
        self.status = JobStatus::Processing { lease_until };
        self.lease_token = Some(lease_token);
        self.lease_until = Some(lease_until);
        self.updated_at = Utc::now();
    }

    /// Complete the job successfully
    pub fn complete(&mut self) {
        self.status = JobStatus::Completed { completed_at: Utc::now() };
        self.lease_token = None;
        self.lease_until = None;
        self.updated_at = Utc::now();
    }

    /// Fail the job permanently
    pub fn fail(&mut self, error: String) {
        self.status = JobStatus::Failed { failed_at: Utc::now(), error: error.clone() };
        self.last_error = Some(error);
        self.lease_token = None;
        self.lease_until = None;
        self.updated_at = Utc::now();
    }

    /// Schedule a retry
    pub fn schedule_retry(&mut self, retry_at: DateTime<Utc>) {
        self.status = JobStatus::Retrying { retry_at };
        self.attempt += 1;
        self.lease_token = None;
        self.lease_until = None;
        self.updated_at = Utc::now();
    }

    /// Cancel the job
    pub fn cancel(&mut self) {
        self.status = JobStatus::Canceled { canceled_at: Utc::now() };
        self.lease_token = None;
        self.lease_until = None;
        self.updated_at = Utc::now();
    }
}

/// A job that has been leased for processing
#[derive(Debug, Clone)]
pub struct LeasedJob {
    /// The job record
    pub record: JobRecord,
    
    /// Lease token for acknowledgment
    pub lease_token: LeaseToken,
    
    /// When the lease expires
    pub lease_until: DateTime<Utc>,
}

impl LeasedJob {
    /// Create a new leased job
    pub fn new(record: JobRecord, lease_token: LeaseToken, lease_until: DateTime<Utc>) -> Self {
        Self {
            record,
            lease_token,
            lease_until,
        }
    }

    /// Get the job ID
    pub fn job_id(&self) -> &JobId {
        &self.record.job_id
    }

    /// Get the job message
    pub fn message(&self) -> &JobMessage {
        &self.record.message
    }

    /// Check if the lease is still valid
    pub fn lease_valid(&self, now: DateTime<Utc>) -> bool {
        self.lease_until > now
    }

    /// Get time remaining on lease
    pub fn lease_remaining(&self, now: DateTime<Utc>) -> chrono::Duration {
        self.lease_until - now
    }
}
