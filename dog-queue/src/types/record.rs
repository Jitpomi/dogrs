use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{JobId, JobMessage, LeaseToken};

/// Job status lifecycle.
///
/// `#[non_exhaustive]` allows new variants to be added in minor releases without
/// breaking downstream `match` expressions.  Downstream crates should always
/// include a `_ =>` catch-all arm.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is queued and waiting to be processed
    Enqueued,

    /// Job is currently being processed by a worker
    Processing { lease_until: DateTime<Utc> },

    /// Job failed and is waiting to be retried
    Retrying { retry_at: DateTime<Utc> },

    /// Job completed successfully
    Completed { completed_at: DateTime<Utc> },

    /// Job failed permanently (max retries exceeded or permanent error)
    Failed {
        failed_at: DateTime<Utc>,
        error: String,
    },

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

    /// Get the status name as a string.
    ///
    /// This match is exhaustive inside the defining crate — `#[non_exhaustive]`
    /// only constrains external matchers.  Adding a new `JobStatus` variant
    /// will cause a compile error here, which is the correct signal to update
    /// the name mapping alongside the new variant.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
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

    /// Current job status.
    ///
    /// When the status is [`JobStatus::Processing`], the `lease_until` timestamp
    /// lives inside the enum payload — it is the single authoritative source.
    /// Use [`Self::lease_until()`] to read it; mutate it via `start_processing`
    /// or `heartbeat_extend` in the backend to keep it consistent.
    pub status: JobStatus,

    /// Current attempt number (starts at 0, incremented by `dequeue`)
    pub attempt: u32,

    /// When the job was created
    pub created_at: DateTime<Utc>,

    /// When the job was last updated
    pub updated_at: DateTime<Utc>,

    /// Last error message (if any)
    pub last_error: Option<String>,

    /// JSON-serialized result returned by the job handler on successful completion.
    ///
    /// Populated by [`QueueBackend::ack_complete`] when the handler returns
    /// `Ok(result)`. Retrieve and deserialize via [`QueueAdapter::get_result`].
    ///
    /// Uses `#[serde(default)]` so that records serialized before this field
    /// was added can be deserialized without error.
    #[serde(default)]
    pub result: Option<String>,

    /// Current lease token (if processing).
    ///
    /// Skipped during serialization to prevent the raw proof-of-ownership token
    /// from appearing in API responses, debug dumps, or webhook payloads.
    /// Lease tokens are session-scoped — persistent backends store them in a
    /// separate lease table, not embedded in the job record.
    #[serde(skip)]
    pub lease_token: Option<LeaseToken>,
}

impl JobRecord {
    /// Create a new job record
    pub fn new(job_id: JobId, tenant_id: impl Into<String>, message: JobMessage) -> Self {
        let now = Utc::now();

        // Always start as Enqueued regardless of run_at.
        // Delayed-job eligibility is enforced by the (priority, run_at, job_id) tuple
        // stored in the queue entry — the dequeue scan gates on run_at <= now there.
        // Setting Scheduled here caused data loss: dequeue phase 2 only leases
        // Enqueued | Retrying, so delayed jobs silently fell into the tombstone arm.
        Self {
            job_id,
            tenant_id: tenant_id.into(),
            message,
            status: JobStatus::Enqueued,
            attempt: 0,
            created_at: now,
            updated_at: now,
            last_error: None,
            result: None,
            lease_token: None,
        }
    }

    /// The lease deadline when this job is currently being processed, or `None`.
    ///
    /// This is the single authoritative source for the lease deadline.
    /// It lives inside [`JobStatus::Processing`] so that mutations via
    /// `heartbeat_extend` update both the eligibility check and the expiry
    /// check in a single write.
    pub fn lease_until(&self) -> Option<DateTime<Utc>> {
        match &self.status {
            JobStatus::Processing { lease_until } => Some(*lease_until),
            _ => None,
        }
    }

    /// Check if the lease has expired
    pub fn lease_expired(&self, now: DateTime<Utc>) -> bool {
        match &self.status {
            JobStatus::Processing { lease_until } => *lease_until < now,
            _ => false,
        }
    }

    /// Set an error and update timestamp
    pub fn set_error(&mut self, error: String) {
        self.last_error = Some(error);
        self.updated_at = Utc::now();
    }

    /// Start processing with a lease.
    ///
    /// The `lease_until` timestamp is stored exclusively inside
    /// [`JobStatus::Processing`] — it is the single source of truth.
    pub fn start_processing(&mut self, lease_token: LeaseToken, lease_until: DateTime<Utc>) {
        self.status = JobStatus::Processing { lease_until };
        self.lease_token = Some(lease_token);
        self.updated_at = Utc::now();
    }

    /// Complete the job successfully
    pub fn complete(&mut self) {
        let now = Utc::now();
        self.status = JobStatus::Completed { completed_at: now };
        self.lease_token = None;
        self.updated_at = now;
    }

    /// Fail the job permanently
    pub fn fail(&mut self, error: String) {
        let now = Utc::now();
        self.status = JobStatus::Failed {
            failed_at: now,
            error: error.clone(),
        };
        self.last_error = Some(error);
        self.lease_token = None;
        self.updated_at = now;
    }

    /// Schedule a retry.
    ///
    /// Does NOT increment `attempt` — that is `dequeue`'s job when the lease is
    /// created, making `dequeue` the sole source of truth for the attempt counter.
    /// Incrementing here AND in `dequeue` would silently halve the retry budget.
    pub fn schedule_retry(&mut self, retry_at: DateTime<Utc>) {
        self.status = JobStatus::Retrying { retry_at };
        self.lease_token = None;
        self.updated_at = Utc::now();
    }

    /// Cancel the job
    pub fn cancel(&mut self) {
        let now = Utc::now();
        self.status = JobStatus::Canceled { canceled_at: now };
        self.lease_token = None;
        self.updated_at = now;
    }
}

/// A job that has been leased for processing
#[derive(Debug, Clone)]
pub struct LeasedJob {
    /// The job record
    pub record: JobRecord,

    /// Lease token for acknowledgment
    pub lease_token: LeaseToken,

    /// Lease expiry captured at dequeue time.
    ///
    /// # Warning — stale after heartbeat extensions
    ///
    /// `LeasedJob` is a point-in-time clone produced by `dequeue`.  Each
    /// successful `heartbeat_extend` call updates the lease deadline in the
    /// backend, but this field is **never refreshed**.  After a heartbeat
    /// extension, `lease_until` reflects the original dequeue-time expiry,
    /// not the current backend deadline.
    ///
    /// To check whether a lease is still alive after heartbeating, compare
    /// the current time against the deadline returned by the most recent
    /// `heartbeat_extend` call — not this field.
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

    /// Check if the lease is still valid.
    ///
    /// > **Warning — stale after heartbeat extensions**
    ///
    /// `lease_until` is a point-in-time snapshot captured at `dequeue` time.
    /// After any successful `heartbeat_extend` call the backend deadline is
    /// extended, but this field is **never refreshed**. Comparing against it
    /// after a heartbeat silently returns `false` for a lease that is alive.
    ///
    /// Prefer tracking the `new_lease_until` value from the most recent
    /// `heartbeat_extend` call for post-heartbeat validity checks.
    pub fn lease_valid(&self, now: DateTime<Utc>) -> bool {
        self.lease_until > now
    }

    /// Time remaining on the lease, or `None` if expired.
    ///
    /// > **Warning — stale after heartbeat extensions**
    ///
    /// Same stale-snapshot caveat as [`lease_valid`](Self::lease_valid).
    pub fn lease_remaining(&self, now: DateTime<Utc>) -> Option<chrono::Duration> {
        let remaining = self.lease_until - now;
        if remaining > chrono::Duration::zero() {
            Some(remaining)
        } else {
            None
        }
    }
}
