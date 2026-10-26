#![cfg(feature = "queue")]
use dog_queue::{
    backend::memory::MemoryBackend, realtime::WatchOptions, JobMessage, QueueBackend, QueueCtx,
};
use dog_transport::queue::queue_sse_stream;
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

#[tokio::test]
async fn sse_reconnect_recovers_completion_and_revocation_closes_stream() {
    let backend = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("customer-a");
    let id = backend
        .enqueue(ctx.clone(), JobMessage::new("test", vec![], "bytes", "q"))
        .await
        .unwrap();
    let (revoke, rx) = watch::channel(false);
    let stream = queue_sse_stream(
        backend.clone(),
        ctx.clone(),
        vec![id.clone()],
        WatchOptions::default(),
        rx.clone(),
    )
    .unwrap();
    let mut stream = Box::pin(stream);
    let first = stream.next().await.unwrap();
    assert_eq!(first.event, Some("dogrs.job_status"));
    assert!(first.data.contains("enqueued"));
    drop(stream);
    backend.cancel(ctx.clone(), id.clone()).await.unwrap();
    let mut stream = Box::pin(
        queue_sse_stream(
            backend.clone(),
            ctx,
            vec![id.clone()],
            WatchOptions::default(),
            rx.clone(),
        )
        .unwrap(),
    );
    assert!(stream.next().await.unwrap().data.contains("canceled"));
    revoke.send(true).unwrap();
    assert!(stream.next().await.unwrap().data.contains("revoked"));
    assert!(stream.next().await.is_none());
    let (_revoke2, rx2) = watch::channel(false);
    let mut wrong = Box::pin(
        queue_sse_stream(
            backend,
            QueueCtx::new("customer-b"),
            vec![id],
            WatchOptions::default(),
            rx2,
        )
        .unwrap(),
    );
    let error = wrong.next().await.unwrap();
    assert_eq!(error.event, Some("dogrs.stream_error"));
    assert_eq!(
        error.data,
        "{\"reason\":\"resync_required\",\"resync_required\":true}"
    );
    assert!(wrong.next().await.is_none());
}

#[tokio::test]
async fn real_http_sse_delivers_status_without_leaving_detached_workers() {
    use axum::{
        response::sse::{Event, Sse},
        routing::get,
        Router,
    };
    let backend = Arc::new(MemoryBackend::new());
    let ctx = QueueCtx::new("verified-tenant");
    let id = backend
        .enqueue(ctx.clone(), JobMessage::new("test", vec![], "bytes", "q"))
        .await
        .unwrap();
    let (revoke, rx) = watch::channel(false);
    let app = Router::new().route(
        "/events",
        get(move || {
            let stream = queue_sse_stream(
                backend.clone(),
                ctx.clone(),
                vec![id.clone()],
                WatchOptions {
                    lifetime: Duration::from_millis(100),
                    ..Default::default()
                },
                rx.clone(),
            )
            .unwrap();
            async move {
                Sse::new(stream.map(|e| {
                    Ok::<_, std::convert::Infallible>(
                        Event::default().event(e.event.unwrap()).data(e.data),
                    )
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let body = reqwest::Client::new()
        .get(format!("http://{address}/events"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("event: dogrs.job_status"));
    assert!(body.contains("enqueued"));
    assert!(body.contains("expired"));
    drop(revoke);
    server.abort();
}

#[tokio::test]
async fn event_hub_receives_authorized_snapshots_and_revocation_stops_delivery() {
    use dog_core::{
        DogAppBuilder, HookContext, ServiceCaller, ServiceEventData, ServiceEventKind,
        ServiceMethodKind, TenantContext,
    };
    let backend = Arc::new(MemoryBackend::new());
    let id = backend
        .enqueue(
            QueueCtx::new("a"),
            JobMessage::new("j", vec![], "json", "q"),
        )
        .await
        .unwrap();
    let second = backend
        .enqueue(
            QueueCtx::new("a"),
            JobMessage::new("second", vec![], "json", "q"),
        )
        .await
        .unwrap();
    let mut builder = DogAppBuilder::<String, ()>::new();
    let (tx, rx) = watch::channel(false);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    builder.on(
        "queue",
        ServiceEventKind::custom("job_status"),
        Arc::new(move |data, ctx| {
            assert_eq!(ctx.tenant.tenant_id.0, "a");
            let ServiceEventData::Custom(payload) = data else {
                panic!("expected custom event")
            };
            assert_eq!(
                payload
                    .downcast_ref::<dog_queue::realtime::JobUpdate>()
                    .unwrap()
                    .status,
                "enqueued"
            );
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tx.send(true).unwrap();
            Box::pin(async { Ok(()) })
        }),
    );
    let app = builder.build();
    let context = HookContext::new(
        TenantContext::new("a"),
        ServiceMethodKind::Find,
        (),
        ServiceCaller::new(app.clone()),
        app.config_snapshot(),
    );
    dog_transport::queue::forward_queue_events(
        &app,
        &context,
        backend,
        vec![id, second],
        WatchOptions::default(),
        rx,
    )
    .await
    .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}
