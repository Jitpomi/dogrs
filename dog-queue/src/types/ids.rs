use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a job
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// Convert a `String` into a `JobId`.
///
/// # Panics
///
/// Panics if `id` is empty.  An empty `JobId` would never match any stored
/// job, producing a confusing `JobNotFound` error far from the construction
/// site.  Prefer [`JobId::new`] for fresh IDs or validate the source string
/// before calling this.
impl From<String> for JobId {
    fn from(id: String) -> Self {
        assert!(
            !id.is_empty(),
            "JobId::from called with an empty string — this will never match any stored job"
        );
        Self(id)
    }
}

/// Convert a `&str` into a `JobId`.
///
/// # Panics
///
/// Panics if `id` is empty.  See [`From<String>`](JobId#impl-From<String>) for details.
impl From<&str> for JobId {
    fn from(id: &str) -> Self {
        assert!(
            !id.is_empty(),
            "JobId::from called with an empty string — this will never match any stored job"
        );
        Self(id.to_string())
    }
}

/// Lease token for job processing - prevents concurrent processing
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

impl Default for LeaseToken {
    fn default() -> Self {
        Self::new()
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
    ///
    /// Uses **char-aware** slicing (`chars().take(N)`) rather than raw byte
    /// indexing (`&s[..4]`) to avoid a panic when the token contains multi-byte
    /// UTF-8 codepoints.  UUID tokens are always ASCII-safe, but `From<String>`
    /// accepts any non-empty string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0.as_str();
        // Use char count, not byte length, to handle multi-byte UTF-8 safely.
        let char_count = s.chars().count();
        if char_count > 8 {
            let prefix: String = s.chars().take(4).collect();
            // Reverse, take 4 from the back, reverse again to restore order.
            let suffix: String = s
                .chars()
                .rev()
                .take(4)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            write!(f, "{prefix}\u{2026}{suffix}") // U+2026 HORIZONTAL ELLIPSIS
        } else {
            // Shorter than expected (not a UUID) — redact entirely.
            write!(f, "[redacted]")
        }
    }
}

/// Convert a `String` into a `LeaseToken`.
///
/// # Panics
///
/// Panics if `token` is empty.  An empty `LeaseToken` would never match any
/// active lease, producing a confusing `InvalidLeaseToken` error far from the
/// construction site.  Prefer [`LeaseToken::new`] for fresh tokens.
impl From<String> for LeaseToken {
    fn from(token: String) -> Self {
        assert!(
            !token.is_empty(),
            "LeaseToken::from called with an empty string — this will never match any active lease"
        );
        Self(token)
    }
}

/// Convert a `&str` into a `LeaseToken`.
///
/// # Panics
///
/// Panics if `token` is empty.  See [`From<String>`](LeaseToken#impl-From<String>) for details.
impl From<&str> for LeaseToken {
    fn from(token: &str) -> Self {
        assert!(
            !token.is_empty(),
            "LeaseToken::from called with an empty string — this will never match any active lease"
        );
        Self(token.to_string())
    }
}
