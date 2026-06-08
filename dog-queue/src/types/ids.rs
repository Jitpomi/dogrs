use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a job
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    /// Generate a new unique job ID
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
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
pub struct LeaseToken(String);

impl LeaseToken {
    /// Generate a new unique lease token
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}



impl fmt::Display for LeaseToken {
    /// Displays a **redacted** form of the token to prevent leakage in logs,
    /// error messages, and tracing spans.
    ///
    /// The full token value is the proof-of-ownership for a job's processing
    /// claim — logging it verbatim would allow anyone with log access to replay
    /// it and call `ack_complete`/`ack_fail` on jobs they do not own.
    ///
    /// Use [`LeaseToken::as_str`] only when the raw value is genuinely required
    /// (e.g. for direct backend comparison).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0.as_str();
        if s.len() > 8 {
            write!(f, "{}…{}", &s[..4], &s[s.len() - 4..])
        } else {
            // Shorter than expected (not a UUID) — redact entirely.
            write!(f, "[redacted]")
        }
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
