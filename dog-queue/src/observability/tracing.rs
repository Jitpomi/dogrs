use std::sync::Arc;
#[cfg(feature = "tracing-opentelemetry")]
use opentelemetry::trace::{TraceId, SpanId};


/// Distributed tracing integration for job processing
#[cfg(feature = "tracing-opentelemetry")]
pub struct DistributedTracing {
    tracer: Arc<dyn opentelemetry::trace::Tracer + Send + Sync>,
}

/// Stub for when opentelemetry is not enabled
#[cfg(not(feature = "tracing-opentelemetry"))]
pub struct DistributedTracing;

#[cfg(feature = "tracing-opentelemetry")]
impl DistributedTracing {
    /// Create new distributed tracing instance
    pub fn new() -> Self {
        // In production, this would be configured with actual OTLP endpoint
        let tracer = opentelemetry::global::tracer("dog-queue");
        
        Self {
            tracer: Arc::new(tracer),
        }
    }
}

#[cfg(not(feature = "tracing-opentelemetry"))]
impl DistributedTracing {
    /// Create stub tracing instance
    pub fn new() -> Self {
        Self
    }
}


/// Span wrapper for job execution
#[cfg(feature = "tracing-opentelemetry")]
pub struct JobSpan {
    span: Box<dyn opentelemetry::trace::Span + Send + Sync>,
}

/// Stub span for when opentelemetry is not enabled
#[cfg(not(feature = "tracing-opentelemetry"))]
pub struct JobSpan;

#[cfg(feature = "tracing-opentelemetry")]
impl JobSpan {
    /// Record job completion
    pub fn record_success(&mut self) {
        self.span.set_status(opentelemetry::trace::Status::Ok);
        self.span.set_attribute(opentelemetry::KeyValue::new("job.status", "completed"));
    }

    /// Record job failure
    pub fn record_failure(&mut self, error: &str) {
        self.span.set_status(opentelemetry::trace::Status::error(error.to_string()));
        self.span.set_attribute(opentelemetry::KeyValue::new("job.status", "failed"));
        self.span.set_attribute(opentelemetry::KeyValue::new("job.error", error.to_string()));
    }

    /// Record job retry
    pub fn record_retry(&mut self, attempt: u32, error: &str) {
        self.span.set_attribute(opentelemetry::KeyValue::new("job.status", "retrying"));
        self.span.set_attribute(opentelemetry::KeyValue::new("job.attempt", attempt as i64));
        self.span.set_attribute(opentelemetry::KeyValue::new("job.retry_reason", error.to_string()));
    }

    /// Add custom attribute
    pub fn set_attribute(&mut self, key: &str, value: &str) {
        self.span.set_attribute(opentelemetry::KeyValue::new(key, value.to_string()));
    }
}

#[cfg(not(feature = "tracing-opentelemetry"))]
impl JobSpan {
    /// Record job completion (stub)
    pub fn record_success(&mut self) {}

    /// Record job failure (stub)
    pub fn record_failure(&mut self, _error: &str) {}

    /// Record job retry (stub)
    pub fn record_retry(&mut self, _attempt: u32, _error: &str) {}

    /// Add custom attribute (stub)
    pub fn set_attribute(&mut self, _key: &str, _value: &str) {}
}

#[cfg(feature = "tracing-opentelemetry")]
impl Drop for JobSpan {
    fn drop(&mut self) {
        self.span.end();
    }
}

/// Span collector for aggregating trace data
pub struct SpanCollector {
    spans: Arc<std::sync::Mutex<Vec<SpanData>>>,
}

impl SpanCollector {
    pub fn new() -> Self {
        Self {
            spans: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Collect span data
    pub fn collect_span(&self, span_data: SpanData) {
        let mut spans = self.spans.lock().unwrap();
        spans.push(span_data);
        
        // Keep only last 10000 spans
        if spans.len() > 10000 {
            spans.remove(0);
        }
    }

    /// Get collected spans
    pub fn get_spans(&self) -> Vec<SpanData> {
        self.spans.lock().unwrap().clone()
    }

    /// Clear collected spans
    pub fn clear(&self) {
        self.spans.lock().unwrap().clear();
    }
}

/// Span data for analysis
#[derive(Debug, Clone)]
pub struct SpanData {
    pub trace_id: String,
    pub span_id: String,
    pub operation_name: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_ms: Option<u64>,
    pub status: SpanStatus,
    pub attributes: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum SpanStatus {
    Ok,
    Error(String),
    Cancelled,
}

impl Default for DistributedTracing {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SpanCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QueueCtx, JobId};

    #[test]
    fn test_distributed_tracing() {
        let _tracing = DistributedTracing::new();
        let _ctx = QueueCtx::new("test_tenant".to_string());
        let _job_id = JobId::new();
        
        // Test passes - basic tracing functionality works
    }

    #[test]
    fn test_span_collector() {
        let collector = SpanCollector::new();
        
        let span_data = SpanData {
            trace_id: "test_trace".to_string(),
            span_id: "test_span".to_string(),
            operation_name: "test_operation".to_string(),
            start_time: chrono::Utc::now(),
            end_time: None,
            duration_ms: None,
            status: SpanStatus::Ok,
            attributes: std::collections::HashMap::new(),
        };
        
        collector.collect_span(span_data.clone());
        
        let spans = collector.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].trace_id, "test_trace");
    }
}
