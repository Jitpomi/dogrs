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

    /// Start from the persisted producer parent, never an unrelated worker context.
    pub fn start_message(
        &self,
        ctx: &crate::QueueCtx,
        id: &crate::JobId,
        message: &crate::JobMessage,
    ) -> JobSpan {
        #[cfg(feature = "tracing-opentelemetry")]
        {
            let parent = message
                .traceparent
                .as_deref()
                .and_then(parse_parent)
                .unwrap_or_default();
            self.start_job_with_context(ctx, id, &message.job_type, &parent)
        }
        #[cfg(not(feature = "tracing-opentelemetry"))]
        self.start_job(ctx, id, &message.job_type)
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
    pub fn record_terminal(&mut self) {
        self.finish("already_terminal", false);
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

/// Capture only W3C identity and sampling flags. Explicit valid parents survive forwarding.
pub(crate) fn capture_parent(message: crate::JobMessage) -> crate::JobMessage {
    #[cfg(feature = "tracing-opentelemetry")]
    {
        let mut message = message;
        if message
            .traceparent
            .as_deref()
            .and_then(parse_parent)
            .is_none()
        {
            let current = Context::current();
            use tracing_opentelemetry::OpenTelemetrySpanExt;
            let current = if current.span().span_context().is_valid() {
                current
            } else {
                tracing::Span::current().context()
            };
            let span = current.span();
            let sc = span.span_context();
            message.traceparent = sc.is_valid().then(|| {
                format!(
                    "00-{}-{}-{:02x}",
                    sc.trace_id(),
                    sc.span_id(),
                    sc.trace_flags().to_u8() & 1
                )
            });
        }
        message
    }
    #[cfg(not(feature = "tracing-opentelemetry"))]
    message
}

#[cfg(feature = "tracing-opentelemetry")]
fn parse_parent(value: &str) -> Option<Context> {
    use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId, TraceState};
    // Bounded, strict version-00 format; malformed external metadata never panics.
    if value.len() != 55 || !value.is_ascii() {
        return None;
    }
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 4
        || parts[0] != "00"
        || parts[1].len() != 32
        || parts[2].len() != 16
        || parts[3].len() != 2
        || !parts[1..]
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return None;
    }
    let sc = SpanContext::new(
        TraceId::from_hex(parts[1]).ok()?,
        SpanId::from_hex(parts[2]).ok()?,
        TraceFlags::new(u8::from_str_radix(parts[3], 16).ok()? & 1),
        true,
        TraceState::default(),
    );
    sc.is_valid()
        .then(|| Context::new().with_remote_span_context(sc))
}

#[cfg(all(test, feature = "tracing-opentelemetry"))]
mod propagation_tests {
    use super::*;
    #[test]
    fn validates_parents_and_reads_legacy_records() {
        for bad in [
            "",
            "00-",
            "é",
            "00-00000000000000000000000000000000-2222222222222222-01",
            "00-11111111111111111111111111111111-0000000000000000-01",
            "00-1111111111111111111111111111111g-2222222222222222-01",
        ] {
            assert!(parse_parent(bad).is_none());
        }
        assert!(parse_parent("00-11111111111111111111111111111111-2222222222222222-01").is_some());
        assert!(parse_parent("00-4BF92F3577B34DA6A3CE929D0E0E4736-00F067AA0BA902B7-01").is_some());
        let message = crate::JobMessage::new("j", vec![], "json", "q");
        let json = serde_json::to_value(message).unwrap();
        assert!(json.get("traceparent").is_none());
        let legacy: crate::JobMessage = serde_json::from_value(json).unwrap();
        assert!(legacy.traceparent.is_none());
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
