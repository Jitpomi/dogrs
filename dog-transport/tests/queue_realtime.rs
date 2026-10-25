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
