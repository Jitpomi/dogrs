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

    #[error("Job execution failed: {0}")]
    JobFailed(#[from] JobError),

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

    #[error("Worker shutdown")]
    WorkerShutdown,

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
        Self::SerializationError(err.to_string())
    }
}
