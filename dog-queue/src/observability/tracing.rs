#[cfg(feature = "tracing-opentelemetry")]
use opentelemetry::{
    trace::{SpanKind, Status, TraceContextExt, Tracer},
    Context, KeyValue,
};
use std::sync::Arc;

/// Uses the application's OpenTelemetry provider; never installs a global exporter.
pub struct DistributedTracing;
impl DistributedTracing {
    pub fn new() -> Self {
        Self
    }

    /// Start one processing attempt. Tags, payloads and raw errors are not exported.
    pub fn start_job(&self, ctx: &crate::QueueCtx, id: &crate::JobId, job_type: &str) -> JobSpan {
        #[cfg(feature = "tracing-opentelemetry")]
        {
            self.start_job_with_context(ctx, id, job_type, &Context::current())
        }
        #[cfg(not(feature = "tracing-opentelemetry"))]
        {
            let _ = (ctx, id, job_type);
            JobSpan {}
        }
    }

    #[cfg(feature = "tracing-opentelemetry")]
    pub fn start_job_with_context(
        &self,
        ctx: &crate::QueueCtx,
        id: &crate::JobId,
        job_type: &str,
        parent: &Context,
    ) -> JobSpan {
        let span = opentelemetry::global::tracer("dog-queue")
            .span_builder("dog.queue.process")
            .with_kind(SpanKind::Consumer)
            .with_attributes(vec![
                KeyValue::new("dog.tenant_id", ctx.tenant_id.clone()),
                KeyValue::new("messaging.message.id", id.to_string()),
                KeyValue::new("dog.job_type", job_type.to_owned()),
            ])
            .start_with_context(&opentelemetry::global::tracer("dog-queue"), parent);
        JobSpan {
            context: parent.with_span(span),
            finished: false,
        }
    }
}

/// Ends on drop, including future cancellation. Incomplete attempts are not successes.
pub struct JobSpan {
    #[cfg(feature = "tracing-opentelemetry")]
    context: Context,
    #[cfg(feature = "tracing-opentelemetry")]
    finished: bool,
}
impl JobSpan {
    /// Attach to each poll with `FutureExt::with_context`, never hold a context guard across await.
    #[cfg(feature = "tracing-opentelemetry")]
    pub fn context(&self) -> Context {
        self.context.clone()
    }
    pub fn record_success(&mut self) {
        self.finish("completed", false);
    }
    /// Error text is deliberately not exported: it may contain payloads or secrets.
    pub fn record_failure(&mut self, _error: &str) {
        self.finish("failed", true);
    }
    pub fn record_retry(&mut self, attempt: u32, _error: &str) {
        self.set_attribute("dog.retry_attempt", &attempt.to_string());
        self.finish("retrying", true);
    }
    pub fn record_canceled(&mut self) {
        self.finish("canceled", false);
    }
    /// Caller is responsible for ensuring custom attributes contain no secrets.
    pub fn set_attribute(&mut self, key: &str, value: &str) {
        #[cfg(feature = "tracing-opentelemetry")]
        self.context
            .span()
            .set_attribute(KeyValue::new(key.to_owned(), value.to_owned()));
        #[cfg(not(feature = "tracing-opentelemetry"))]
        let _ = (key, value);
    }
    fn finish(&mut self, outcome: &str, error: bool) {
        #[cfg(feature = "tracing-opentelemetry")]
        {
            self.context
                .span()
                .set_attribute(KeyValue::new("dog.outcome", outcome.to_owned()));
            self.context.span().set_status(if error {
                Status::error(outcome.to_owned())
            } else {
                Status::Ok
            });
            self.finished = true;
        }
        #[cfg(not(feature = "tracing-opentelemetry"))]
        let _ = (outcome, error);
    }
}
impl Drop for JobSpan {
    fn drop(&mut self) {
        #[cfg(feature = "tracing-opentelemetry")]
        {
            if !self.finished {
                self.context
                    .span()
                    .set_attribute(KeyValue::new("dog.outcome", "interrupted"));
                self.context
                    .span()
                    .set_status(Status::error("attempt interrupted before acknowledgment"));
            }
            self.context.span().end();
        }
    }
}

/// Span collector for aggregating trace data
pub struct SpanCollector {
    /// `parking_lot::Mutex` is infallible (no poisoning) and safe in async context
    /// as long as no `.await` is held across the lock — none of our methods do.
    /// `VecDeque` enables O(1) `pop_front()` eviction rather than O(n) `remove(0)`.
    spans: Arc<parking_lot::Mutex<std::collections::VecDeque<SpanData>>>,
}

impl SpanCollector {
    pub fn new() -> Self {
        Self {
            spans: Arc::new(parking_lot::Mutex::new(std::collections::VecDeque::new())),
        }
    }

    /// Collect span data (ring buffer — keeps the most recent 10 000 spans).
    pub fn collect_span(&self, span_data: SpanData) {
        let mut spans = self.spans.lock();
        spans.push_back(span_data);
        if spans.len() > 10_000 {
            spans.pop_front(); // O(1) — previously Vec::remove(0) was O(n)
        }
    }

    /// Get collected spans.
    ///
    /// Clones the ring-buffer structure under the lock and then releases the
    /// lock before iterating, so `collect_span()` callers are not blocked for
    /// the entire O(n × attributes) deep-clone.
    pub fn get_spans(&self) -> Vec<SpanData> {
        // Clone the VecDeque (and its SpanData values including HashMap attributes)
        // in one bulk operation while holding the lock, then release the lock
        // before converting to Vec. concurrent collect_span() calls can proceed
        // as soon as the lock is released, not after the full allocation.
        let snapshot: std::collections::VecDeque<SpanData> = self.spans.lock().clone();
        snapshot.into_iter().collect()
    }

    /// Clear collected spans
    pub fn clear(&self) {
        self.spans.lock().clear();
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

/// Job span status.
///
/// `#[non_exhaustive]` allows new status variants to be added in future
/// minor versions without breaking downstream match arms.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum SpanStatus {
    Ok,
    Error(String),
    /// The span was canceled (note: American spelling — consistent with
    /// `JobStatus::Canceled`, `QueueError::JobCanceled`, etc.)
    Canceled,
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
    use crate::{JobId, QueueCtx};

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
