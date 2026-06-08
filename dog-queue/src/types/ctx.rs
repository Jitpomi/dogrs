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
    pub fn new(tenant_id: impl Into<String>) -> Self {
        let tenant_id = tenant_id.into();
        assert!(
            !tenant_id.is_empty(),
            "QueueCtx tenant_id cannot be empty — an empty tenant_id matches all records \
             whose tenant_id is also empty, bypassing multi-tenant isolation"
        );
        Self {
            tenant_id,
            trace_id: None,
            request_id: None,
            tags: HashMap::new(),
        }
    }

    /// Add a trace ID for distributed tracing
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Add a request ID for request correlation
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Add a single observability tag.
    ///
    /// Accepts any `impl Into<String>` for both key and value to avoid
    /// unnecessary `.to_string()` calls at the call site.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add multiple tags at once
    pub fn with_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// Get a tag value by key.
    ///
    /// Returns `Option<&str>` rather than `Option<&String>` — idiomatic Rust;
    /// avoids the `clippy::ref_option_string` anti-pattern and composes directly
    /// with `unwrap_or("default")` and similar `&str` idioms.
    pub fn get_tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(|s| s.as_str())
    }

    /// Check if a tag exists
    pub fn has_tag(&self, key: &str) -> bool {
        self.tags.contains_key(key)
    }

    /// Create a scoped idempotency key for this tenant/queue/job_type combination.
    ///
    /// Uses `\x1f` (ASCII Unit Separator, U+001F) as the component delimiter.
    /// This character cannot appear in well-formed tenant IDs, queue names, job
    /// type names, or user-supplied keys, so components with any printable content
    /// (including `:`, `/`, `_`, etc.) produce unambiguous, collision-free keys.
    ///
    /// Using `:` as the delimiter caused key collisions when component values
    /// contained colons (e.g. `tenant="a:b"`, `queue="c"` produced the same key
    /// as `tenant="a"`, `queue="b:c"`).
    pub fn scoped_idempotency_key(&self, queue: &str, job_type: &str, key: &str) -> String {
        format!("{}\x1f{}\x1f{}\x1f{}", self.tenant_id, queue, job_type, key)
    }
}
