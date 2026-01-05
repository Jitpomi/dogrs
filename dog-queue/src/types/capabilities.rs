use serde::{Deserialize, Serialize};

/// Backend capabilities - explicit feature detection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    fn default() -> Self {
        Self {
            delayed: true,
            scheduled_at: true,
            cancel: false,
            lease_extend: false,
            priority: false,
            idempotency: true,
            dead_letter_queue: false,
        }
    }
}

impl QueueCapabilities {
    /// Create capabilities with all features enabled
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

    /// Create minimal capabilities (basic enqueue/dequeue only)
    pub fn minimal() -> Self {
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

    /// Check if a specific feature is supported
    pub fn supports(&self, feature: &str) -> bool {
        match feature {
            "delayed" => self.delayed,
            "scheduled_at" => self.scheduled_at,
            "cancel" => self.cancel,
            "lease_extend" => self.lease_extend,
            "priority" => self.priority,
            "idempotency" => self.idempotency,
            "dead_letter_queue" => self.dead_letter_queue,
            _ => false,
        }
    }

    /// Get list of supported features
    pub fn supported_features(&self) -> Vec<&'static str> {
        let mut features = Vec::new();
        
        if self.delayed { features.push("delayed"); }
        if self.scheduled_at { features.push("scheduled_at"); }
        if self.cancel { features.push("cancel"); }
        if self.lease_extend { features.push("lease_extend"); }
        if self.priority { features.push("priority"); }
        if self.idempotency { features.push("idempotency"); }
        if self.dead_letter_queue { features.push("dead_letter_queue"); }
        
        features
    }
}
