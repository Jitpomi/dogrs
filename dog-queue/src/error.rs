use thiserror::Error;

/// Result type for queue operations
pub type QueueResult<T> = Result<T, QueueError>;

/// Infrastructure errors for queue operations
#[derive(Error, Debug, Clone)]
pub enum QueueError {
    #[error("Job not found: {0}")]
    JobNotFound(String),

    #[error("Invalid lease token")]
    InvalidLeaseToken,

    #[error("Lease has expired")]
    LeaseExpired,

    #[error("Job has been canceled")]
    JobCanceled,

    #[error("Job is already in terminal state")]
    JobAlreadyTerminal,

    /// Job execution failed; the inner `JobError` is chained as the error source
    /// so callers can walk the full causal chain via `error.source()`.
    #[error("Job execution failed: {0}")]
    JobFailed(#[source] JobError),

    #[error("Codec not found: {0}")]
    CodecNotFound(String),

    #[error("Payload too large: {size} bytes (max: {max})")]
    PayloadTooLarge { size: usize, max: usize },

    #[error("Backend does not support feature: {0}")]
    BackendUnsupported(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Job type not registered: {0}")]
    JobTypeNotRegistered(String),

    #[error("Job type already registered: {0}")]
    JobTypeAlreadyRegistered(String),

    #[error("Worker shutdown")]
    WorkerShutdown,

    /// A caller-supplied configuration value violates a required invariant.
    ///
    /// Distinct from [`QueueError::Internal`] (unexpected runtime failure) so
    /// that error routing, alerting rules, and middleware can correctly classify
    /// configuration mistakes as programmer errors rather than transient faults.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Job execution outcome - determines retry behavior
#[derive(Error, Debug, Clone)]
pub enum JobError {
    /// Retryable error - will schedule retry if attempts remain
    #[error("Retryable error: {0}")]
    Retryable(String),

    /// Permanent error - fail immediately, no retry
    #[error("Permanent error: {0}")]
    Permanent(String),
}

impl JobError {
    /// Create a retryable error
    pub fn retryable(msg: impl Into<String>) -> Self {
        Self::Retryable(msg.into())
    }

    /// Create a permanent error
    pub fn permanent(msg: impl Into<String>) -> Self {
        Self::Permanent(msg.into())
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }

    /// Get the error message
    pub fn message(&self) -> &str {
        match self {
            Self::Retryable(msg) | Self::Permanent(msg) => msg,
        }
    }
}

impl From<serde_json::Error> for QueueError {
    fn from(err: serde_json::Error) -> Self {
        // Preserve the error category so downstream code and operators can
        // distinguish deterministic failures (Syntax, Eof, Data — no point
        // retrying) from transient failures (Io — worth retrying).
        Self::SerializationError(format!("[{:?}] {err}", err.classify()))
    }
}
