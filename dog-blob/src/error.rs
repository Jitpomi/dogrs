use thiserror::Error;

/// Result type for blob operations
pub type BlobResult<T> = Result<T, BlobError>;

/// Errors that can occur during blob operations
#[derive(Error, Debug)]
pub enum BlobError {
    #[error("Blob not found: {id}")]
    NotFound { id: String },

    #[error("Invalid request: {message}")]
    Invalid { message: String },

    #[error("Operation not supported by this store")]
    Unsupported,

    #[error("Upload session not found: {upload_id}")]
    UploadNotFound { upload_id: String },

    #[error("Upload failed: {reason}")]
    UploadFailed { reason: String },

    #[error("Storage backend error: {source}")]
    Backend {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("Serialization error: {source}")]
    Serialization {
        #[from]
        source: serde_json::Error,
    },
}

impl BlobError {
    /// Create a backend error from any error type
    pub fn backend<E>(error: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Backend {
            source: Box::new(error),
        }
    }

    /// Create an invalid request error
    pub fn invalid<S: Into<String>>(message: S) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }

    /// Create a not found error
    pub fn not_found<S: Into<String>>(id: S) -> Self {
        Self::NotFound { id: id.into() }
    }

    /// Create an upload not found error
    pub fn upload_not_found<S: Into<String>>(upload_id: S) -> Self {
        Self::UploadNotFound {
            upload_id: upload_id.into(),
        }
    }

    /// Create an upload failed error
    pub fn upload_failed<S: Into<String>>(reason: S) -> Self {
        Self::UploadFailed {
            reason: reason.into(),
        }
    }
}
