use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::JobPriority;

/// Job message - immutable submission data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobMessage {
    /// Job type identifier for dispatch
    pub job_type: String,

    /// Serialized job payload (opaque bytes)
    pub payload_bytes: Vec<u8>,

    /// Codec used for serialization
    pub codec: String,

    /// Target queue name
    pub queue: String,

    /// Job priority for ordering
    pub priority: JobPriority,

    /// Maximum retry attempts
    pub max_retries: u32,

    /// Absolute UTC eligibility time, or [`Self::IMMEDIATE`] for immediate work.
    pub run_at: DateTime<Utc>,

    /// Optional idempotency key (scoped by tenant/queue/job_type)
    pub idempotency_key: Option<String>,
}

impl JobMessage {
    /// Clock-independent marker for immediate work (also usable for an immediate
    /// retry). Backends may resolve it to their authoritative enqueue time.
    pub const IMMEDIATE: DateTime<Utc> = DateTime::UNIX_EPOCH;

    /// Create a job eligible immediately, independent of the producer's clock.
    pub fn new(
        job_type: impl Into<String>,
        payload_bytes: Vec<u8>,
        codec: impl Into<String>,
        queue: impl Into<String>,
    ) -> Self {
        Self {
            job_type: job_type.into(),
            payload_bytes,
            codec: codec.into(),
            queue: queue.into(),
            priority: JobPriority::default(),
            max_retries: 3,
            run_at: Self::IMMEDIATE,
            idempotency_key: None,
        }
    }

    /// Set the job priority
    pub fn with_priority(mut self, priority: JobPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set the maximum retry attempts
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set when the job should run
    pub fn with_run_at(mut self, run_at: DateTime<Utc>) -> Self {
        self.run_at = run_at;
        self
    }

    /// Set the idempotency key
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Check if the job is eligible to run at the given reference time.
    ///
    /// Takes an explicit `now` rather than calling `Utc::now()` internally so
    /// that callers control the reference timestamp. This makes eligibility checks
    /// deterministic in tests and consistent with [`JobRecord::is_eligible`].
    pub fn is_eligible(&self, now: DateTime<Utc>) -> bool {
        self.run_at <= now
    }

    /// Get the payload size in bytes
    pub fn payload_size(&self) -> usize {
        self.payload_bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_submission_is_eligible_on_a_slower_clock_but_explicit_schedule_is_not() {
        let reference = Utc::now() - chrono::Duration::hours(1);
        let immediate = JobMessage::new("clock", vec![], "bytes", "q");
        assert!(immediate.is_eligible(reference));
        let round_trip: JobMessage =
            serde_json::from_str(&serde_json::to_string(&immediate).unwrap()).unwrap();
        assert!(round_trip.is_eligible(reference));
        let scheduled = immediate.with_run_at(reference + chrono::Duration::minutes(1));
        assert!(!scheduled.is_eligible(reference));
    }
}
