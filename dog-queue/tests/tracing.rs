#![cfg(feature = "tracing-opentelemetry")]
use dog_queue::{observability::tracing::DistributedTracing, JobId, QueueCtx};
use opentelemetry::{
    trace::{FutureExt, Status, TraceContextExt, Tracer, TracerProvider},
    Context,
};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

#[tokio::test]
async fn exports_real_parented_spans_with_outcomes_and_no_raw_errors() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    opentelemetry::global::set_tracer_provider(provider.clone());
    let tracer = provider.tracer("test");
    let parent = Context::new().with_span(tracer.start("request"));
    let parent_id = parent.span().span_context().span_id();
    let tracing = DistributedTracing::new();
    for outcome in [
        "completed",
        "failed",
        "retrying",
        "canceled",
        "already_terminal",
        "interrupted",
    ] {
        let mut span = tracing.start_job_with_context(
            &QueueCtx::new("tenant"),
            &JobId::new(),
            "test",
            &parent,
        );
        async {
            tokio::task::yield_now().await;
            assert_eq!(
                Context::current().span().span_context().trace_id(),
                parent.span().span_context().trace_id()
            );
        }
        .with_context(span.context())
        .await;
        match outcome {
            "completed" => span.record_success(),
            "failed" => span.record_failure("secret-password"),
            "retrying" => span.record_retry(2, "secret-password"),
            "canceled" => span.record_canceled(),
            "already_terminal" => span.record_terminal(),
            _ => (),
        }
        drop(span);
    }
    provider.force_flush().unwrap();
    let spans = exporter.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 6);
    for span in &spans {
        assert_eq!(span.parent_span_id, parent_id);
        assert!(span.span_context.is_valid());
        assert!(!format!("{span:?}").contains("secret-password"));
    }
    assert_eq!(spans[0].status, Status::Ok);
    assert!(matches!(spans[1].status, Status::Error { .. }));
    assert_eq!(spans[3].status, Status::Ok);
    assert_eq!(spans[4].status, Status::Ok);
    assert!(matches!(spans[5].status, Status::Error { .. }));
    assert!(!Context::current().span().span_context().is_valid());
    let adapter = dog_queue::QueueAdapter::new(dog_queue::backend::memory::MemoryBackend::new());
    adapter.register_job::<TracedJob>().await.unwrap();
    adapter
        .enqueue(QueueCtx::new("worker-tenant"), TracedJob)
        .with_context(parent.clone())
        .await
        .unwrap();
    let handle = adapter
        .start_workers(
            QueueCtx::new("worker-tenant"),
            (),
            vec!["traced-job".into()],
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if exporter.get_finished_spans().unwrap().iter().any(|s| {
                s.attributes
                    .iter()
                    .any(|a| a.key.as_str() == "dog.job_type" && a.value.as_str() == "traced-job")
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    handle.shutdown().await.unwrap();
    let exported = exporter.get_finished_spans().unwrap();
    let attempt = exported
        .iter()
        .find(|s| {
            s.attributes
                .iter()
                .any(|a| a.key.as_str() == "dog.job_type" && a.value.as_str() == "traced-job")
        })
        .unwrap();
    assert_eq!(attempt.status, Status::Ok);
    assert_eq!(attempt.parent_span_id, parent_id);
    assert_eq!(
        attempt.span_context.trace_id(),
        parent.span().span_context().trace_id()
    );
    assert!(attempt
        .attributes
        .iter()
        .any(|a| a.key.as_str() == "dog.outcome" && a.value.as_str() == "completed"));
    parent.span().end();
    provider.shutdown().unwrap();
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct TracedJob;
#[async_trait::async_trait]
impl dog_queue::Job for TracedJob {
    type Context = ();
    type Result = ();
    const JOB_TYPE: &'static str = "traced-job";
    async fn execute(&self, _: ()) -> Result<(), dog_queue::JobError> {
        tokio::task::yield_now().await;
        assert!(Context::current().span().span_context().is_valid());
        dog_queue::realtime::report_progress(50).await.unwrap();
        Ok(())
    }
}
