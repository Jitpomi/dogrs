use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// QueueFeature — type-safe feature identifier
// ---------------------------------------------------------------------------

/// Type-safe feature identifier for capability queries.
///
/// Replaces the previous `supports(&str)` API which silently returned `false`
/// for typos. Use with [`QueueCapabilities::supports`]:
///
/// ```
/// use dog_queue::types::{QueueCapabilities, QueueFeature};
/// let caps = QueueCapabilities::all();
/// assert!(caps.supports(QueueFeature::Cancel));
/// ```
///
/// Serializes as snake_case strings matching the corresponding field names in
/// [`QueueCapabilities`] (e.g. `QueueFeature::ScheduledAt` → `"scheduled_at"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueFeature {
    /// Delayed job execution (run_at > now)
    Delayed,
    /// Scheduled job execution at specific times
    ScheduledAt,
    /// Job cancellation
    Cancel,
    /// Lease extension (heartbeat)
    LeaseExtend,
    /// Priority-ordered dequeue
    Priority,
    /// Idempotency keys for dedup
    Idempotency,
    /// Dead-letter queue for failed jobs
    DeadLetterQueue,
}

// ---------------------------------------------------------------------------
// QueueCapabilities — backend feature advertisement
// ---------------------------------------------------------------------------

/// Backend feature advertisement.
///
/// Each backend's [`QueueBackend::capabilities`] method returns a value that
/// describes which optional features it actually implements.  Callers should
/// query this before using optional operations (cancel, lease extension, etc.).
///
/// # Default
///
/// `QueueCapabilities::default()` is the **unknown/empty** baseline — all
/// features are `false`.  It is intended as the safe conservative value for a
/// backend that has not explicitly declared its features, **not** as a
/// description of a "typical" backend.  Backends that implement additional
/// features must override [`QueueBackend::capabilities`] explicitly.
///
/// # Forward compatibility
///
/// `#[serde(default)]` on the struct ensures that new capability fields added
/// in future versions deserialize as `false` (conservative baseline) when
/// reading data serialized by an older version — no migration required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueCapabilities {
    /// Support for delayed job execution (run_at > now)
    pub delayed: bool,

    /// Support for scheduled job execution at specific times
    pub scheduled_at: bool,

    /// Support for job cancellation
    pub cancel: bool,

    /// Support for lease extension (heartbeat)
    pub lease_extend: bool,

    /// Support for job priority ordering
    pub priority: bool,

    /// Support for idempotency keys
    pub idempotency: bool,

    /// Support for dead letter queue
    pub dead_letter_queue: bool,
}

impl Default for QueueCapabilities {
    /// Returns the **empty/unknown** capability set (all features `false`).
    ///
    /// This is the safe conservative baseline for backends that do not
    /// explicitly declare their capabilities. It does **not** represent what a
    /// typical backend supports — use [`QueueCapabilities::all`] or construct
    /// a backend-specific value instead.
    fn default() -> Self {
        Self {
            delayed: false,
            scheduled_at: false,
            cancel: false,
            lease_extend: false,
            priority: false,
            idempotency: false,
            dead_letter_queue: false,
        }
    }
}

impl QueueCapabilities {
    /// Create capabilities with all features enabled.
    pub fn all() -> Self {
        Self {
            delayed: true,
            scheduled_at: true,
            cancel: true,
            lease_extend: true,
            priority: true,
            idempotency: true,
            dead_letter_queue: true,
        }
    }

    /// Create minimal capabilities (basic enqueue/dequeue only).
    ///
    /// This is identical to [`Default::default()`] (all features `false`).
    /// Prefer `QueueCapabilities::default()` directly for clarity.
    #[deprecated(
        note = "Identical to QueueCapabilities::default(). \
                Use Default::default() for the conservative baseline or \
                QueueCapabilities::all() for full feature support."
    )]
    pub fn minimal() -> Self {
        Self::default()
    }

    /// Check if a specific feature is supported.
    ///
    /// Accepts a [`QueueFeature`] value rather than a `&str` so that typos
    /// are a compile error rather than a silent `false` return.
    pub fn supports(&self, feature: QueueFeature) -> bool {
        match feature {
            QueueFeature::Delayed => self.delayed,
            QueueFeature::ScheduledAt => self.scheduled_at,
            QueueFeature::Cancel => self.cancel,
            QueueFeature::LeaseExtend => self.lease_extend,
            QueueFeature::Priority => self.priority,
            QueueFeature::Idempotency => self.idempotency,
            QueueFeature::DeadLetterQueue => self.dead_letter_queue,
        }
    }

    /// Get list of supported features.
    pub fn supported_features(&self) -> Vec<QueueFeature> {
        let all = [
            QueueFeature::Delayed,
            QueueFeature::ScheduledAt,
            QueueFeature::Cancel,
            QueueFeature::LeaseExtend,
            QueueFeature::Priority,
            QueueFeature::Idempotency,
            QueueFeature::DeadLetterQueue,
        ];
        all.into_iter().filter(|f| self.supports(*f)).collect()
    }
}
