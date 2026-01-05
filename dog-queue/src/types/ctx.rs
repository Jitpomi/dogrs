use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Multi-tenant context for queue operations with observability support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueCtx {
    /// Tenant identifier for multi-tenant isolation
    pub tenant_id: String,
    
    /// Optional trace ID for distributed tracing
    pub trace_id: Option<String>,
    
    /// Optional request ID for request correlation
    pub request_id: Option<String>,
    
    /// Additional tags for observability and filtering
    pub tags: HashMap<String, String>,
}

impl QueueCtx {
    /// Create a new queue context with the given tenant ID
    pub fn new(tenant_id: String) -> Self {
        Self {
            tenant_id,
            trace_id: None,
            request_id: None,
            tags: HashMap::new(),
        }
    }

    /// Add a trace ID for distributed tracing
    pub fn with_trace_id(mut self, trace_id: String) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    /// Add a request ID for request correlation
    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    /// Add a tag for observability
    pub fn with_tag(mut self, key: String, value: String) -> Self {
        self.tags.insert(key, value);
        self
    }

    /// Add multiple tags at once
    pub fn with_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// Get a tag value by key
    pub fn get_tag(&self, key: &str) -> Option<&String> {
        self.tags.get(key)
    }

    /// Check if a tag exists
    pub fn has_tag(&self, key: &str) -> bool {
        self.tags.contains_key(key)
    }

    /// Create a scoped idempotency key for this tenant/queue/job_type combination
    pub fn scoped_idempotency_key(&self, queue: &str, job_type: &str, key: &str) -> String {
        format!("{}:{}:{}:{}", self.tenant_id, queue, job_type, key)
    }
}
