use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a job
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

impl JobId {
    /// Generate a new unique job ID
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create a job ID from a string
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for JobId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for JobId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Lease token for job processing - prevents concurrent processing
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeaseToken(pub String);

impl LeaseToken {
    /// Generate a new unique lease token
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create a lease token from a string
    pub fn from_string(token: String) -> Self {
        Self(token)
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LeaseToken {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for LeaseToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for LeaseToken {
    fn from(token: String) -> Self {
        Self(token)
    }
}

impl From<&str> for LeaseToken {
    fn from(token: &str) -> Self {
        Self(token.to_string())
    }
}
