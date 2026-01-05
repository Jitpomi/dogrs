use serde::{Deserialize, Serialize};

/// Job priority levels for queue ordering (Higher values = higher priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum JobPriority {
    /// Low priority jobs (processed last)
    Low = 1,
    
    /// Normal priority jobs (default)
    Normal = 2,
    
    /// High priority jobs (processed first)
    High = 3,
    
    /// Critical priority jobs (processed immediately)
    Critical = 4,
}

// Correct FIFO ordering: jobs.sort_by_key(|r| (Reverse(r.message.priority), r.created_at))
// This ensures:
// - Higher priority jobs first: Critical > High > Normal > Low
// - Within same priority: older jobs first (created_at ascending)

impl Default for JobPriority {
    fn default() -> Self {
        Self::Normal
    }
}

impl JobPriority {
    /// Get all priority levels in order (low to high)
    pub fn all() -> &'static [JobPriority] {
        &[Self::Low, Self::Normal, Self::High, Self::Critical]
    }

    /// Get the numeric value for ordering
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Create from numeric value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Low),
            2 => Some(Self::Normal),
            3 => Some(Self::High),
            4 => Some(Self::Critical),
            _ => None,
        }
    }

    /// Get human-readable name
    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl std::fmt::Display for JobPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::str::FromStr for JobPriority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("Invalid priority: {}", s)),
        }
    }
}
