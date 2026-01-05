use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::JobPriority;

/// Job message - immutable submission data
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    
    /// When the job should be eligible for processing
    pub run_at: DateTime<Utc>,
    
    /// Optional idempotency key (scoped by tenant/queue/job_type)
    pub idempotency_key: Option<String>,
}

impl JobMessage {
    /// Create a new job message
    pub fn new(
        job_type: String,
        payload_bytes: Vec<u8>,
        codec: String,
        queue: String,
    ) -> Self {
        Self {
            job_type,
            payload_bytes,
            codec,
            queue,
            priority: JobPriority::default(),
            max_retries: 3,
            run_at: Utc::now(),
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
    pub fn with_idempotency_key(mut self, key: String) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// Check if the job is eligible to run now
    pub fn is_eligible(&self) -> bool {
        self.run_at <= Utc::now()
    }

    /// Get the payload size in bytes
    pub fn payload_size(&self) -> usize {
        self.payload_bytes.len()
    }
}
