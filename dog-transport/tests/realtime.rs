#![cfg(feature = "http")]
use axum::{
    http::{HeaderMap, StatusCode},
    routing::get,
    Router,
};
use dog_auth::AuthParams;
use dog_core::{
    DogApp, DogAppBuilder, DogMethod, DogParams, DogRequest, DogService, DogTransportKind,
    TenantContext,
};
use dog_transport::{realtime::*, SseOptions, WebSocketOptions, WsPayload};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{broadcast, watch},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

type Params = AuthParams<Value>;
type App = DogApp<Value, Params>;
type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;
struct Echo(Arc<AtomicUsize>);
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl DogService<Value, Params> for Echo {
    async fn create(
        &self,
        tenant: &TenantContext,
        data: Value,
        params: Params,
    ) -> anyhow::Result<Value> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let _active = Active(self.0.clone());
        if data == "stall" {
            std::future::pending::<()>().await;
        }
        if data == "error" {
            anyhow::bail!("internal-secret-123");
        }
        Ok(
            json!({"data":data,"tenant":tenant.tenant_id.0,"provider":params.provider,"authenticated":params.authenticated,"headers":params.headers}),
        )
    }
}
fn app(options: WebSocketOptions) -> (App, Arc<AtomicUsize>) {
    let active = Arc::new(AtomicUsize::new(0));
    let mut builder = DogAppBuilder::new();
    builder.register_service("echo", Arc::new(Echo(active.clone())));
    builder.set("ws.options", Arc::new(options));
    (builder.build(), active)
}
fn command(data: Value) -> String {
    serde_json::to_string(&WsPayload::Request {
        req: DogRequest {
            request_id: Some("test".into()),
            transport: DogTransportKind::Internal,
            service: "echo".into(),
            method: DogMethod::Create,
            id: None,
            tenant: TenantContext::new("forged"),
            params: DogParams::from(HashMap::from([
                ("headers".into(), json!({"authorization":"forged"})),
                ("authenticated".into(), json!(true)),
                ("inner".into(), Value::Null),
            ])),
            payload: Some(data),
            metadata: HashMap::new(),
        },
    })
    .unwrap()
}
dog_transport::declare_ws_adapter!(axum, public_ws, Params);
async fn authorize(app: App, headers: HeaderMap) -> Result<WebSocketSession, StatusCode> {
    // Test identity provider: production callers must verify their own credentials.
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some("https://trusted.example") {
        return Err(StatusCode::FORBIDDEN);
    }
    let tenant = headers
        .get("x-test-tenant")
        .and_then(|v| v.to_str().ok())
        .filter(|v| matches!(*v, "a" | "b"))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let events = app
        .get::<Arc<broadcast::Sender<Value>>>(tenant)
        .map(|tx| tx.subscribe());
    Ok(WebSocketSession {
        tenant: Some(TenantContext::new(tenant)),
        headers: Some(serde_json::Map::from_iter([(
            "authorization".into(),
            json!("verified-credential"),
        )])),
        events,
        revoke: app
            .get::<Arc<watch::Receiver<bool>>>("revoke")
            .map(|r| (*r).clone()),
        expires_at: None,
    })
}
dog_transport::declare_ws_adapter!(axum, private_ws, Params, authorize = authorize);

fn public_events() -> &'static broadcast::Sender<Value> {
    static CHANNEL: std::sync::OnceLock<broadcast::Sender<Value>> = std::sync::OnceLock::new();
    CHANNEL.get_or_init(|| broadcast::channel(8).0)
}
dog_transport::declare_sse_adapter!(
    axum,
    public_sse,
    public_events(),
    SseOptions {
        keep_alive_interval_secs: Some(1),
        ..Default::default()
    }
);
async fn authorize_sse(headers: HeaderMap) -> Result<SseSubscription, StatusCode> {
    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("test-only") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let (tx, rx) = broadcast::channel(1);
    tx.send(json!(1)).unwrap();
    tx.send(json!(2)).unwrap(); // guaranteed gap
    Ok(SseSubscription::new(rx))
}
dog_transport::declare_sse_adapter!(
    axum,
    private_sse,
    authorize = authorize_sse,
    options = SseOptions::default()
);
struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(app: App) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = listener.local_addr().unwrap().to_string();
    let router = Router::new()
        .route("/ws", get(public_ws))
        .route("/private", get(private_ws))
        .route("/sse", get(public_sse))
        .route("/private-sse", get(private_sse))
        .with_state(app);
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, task }
}
async fn connect(server: &Server, tenant: &str) -> Client {
    let mut req = format!("ws://{}/private", server.url)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("origin", "https://trusted.example".parse().unwrap());
    req.headers_mut()
        .insert("x-test-tenant", tenant.parse().unwrap());
    connect_async(req).await.unwrap().0
}
async fn message(client: &mut Client) -> Message {
    timeout(Duration::from_secs(4), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}
async fn text(client: &mut Client) -> Value {
    serde_json::from_str(message(client).await.to_text().unwrap()).unwrap()
}
async fn wait_active(active: &AtomicUsize, expected: usize) {
    timeout(Duration::from_secs(2), async {
        while active.load(Ordering::SeqCst) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn private_ws_authorizes_before_upgrade_and_pins_tenant_and_headers() {
    let (app, _) = app(Default::default());
    let server = server(app).await;
    assert!(connect_async(format!("ws://{}/private", server.url))
        .await
        .is_err());
    let mut client = connect(&server, "a").await;
    client
        .send(Message::Text(command(json!("hello")).into()))
        .await
        .unwrap();
    let response = text(&mut client).await;
    assert_eq!(response["payload"]["tenant"], "a");
    assert_eq!(response["payload"]["provider"], "websocket");
    assert_eq!(response["payload"]["authenticated"], false);
    assert_eq!(
        response["payload"]["headers"]["authorization"],
        "verified-credential"
    );
    client
        .send(Message::Text(command(json!("error")).into()))
        .await
        .unwrap();
    let response = text(&mut client).await;
    assert!(response["error"].is_string());
    assert!(!response.to_string().contains("internal-secret"));
}

#[tokio::test]
async fn private_events_do_not_cross_tenants() {
    let mut builder = DogAppBuilder::<Value, Params>::new();
    let (a, _) = broadcast::channel(8);
    let (b, _) = broadcast::channel(8);
    let a = Arc::new(a);
    let b = Arc::new(b);
    builder.set("a", a.clone());
    builder.set("b", b.clone());
    let server = server(builder.build()).await;
    let mut ca = connect(&server, "a").await;
    let mut cb = connect(&server, "b").await;
    a.send(json!({"event":"private","data":"a-only"})).unwrap();
    b.send(json!({"event":"private","data":"b-only"})).unwrap();
    assert_eq!(text(&mut ca).await["payload"], "a-only");
    assert_eq!(text(&mut cb).await["payload"], "b-only");
    assert!(timeout(Duration::from_millis(100), ca.next())
        .await
        .is_err());
    assert!(timeout(Duration::from_millis(100), cb.next())
        .await
        .is_err());
}

#[tokio::test]
async fn stalled_request_keeps_heartbeats_alive_then_times_out_and_cancels() {
    let (app, active) = app(WebSocketOptions {
        heartbeat_interval_secs: Some(1),
        request_timeout_secs: Some(2),
        ..Default::default()
    });
    let server = server(app).await;
    let mut client = connect(&server, "a").await;
    client
        .send(Message::Text(command(json!("stall")).into()))
        .await
        .unwrap();
    wait_active(&active, 1).await;
    assert!(matches!(message(&mut client).await, Message::Ping(_)));
    client.flush().await.unwrap(); // tungstenite queues the matching Pong
    let response = loop {
        let frame = message(&mut client).await;
        if let Message::Text(text) = frame {
            break text;
        }
        client.flush().await.unwrap();
    };
    assert!(response.contains("Request timed out"));
    wait_active(&active, 0).await;
    client
        .send(Message::Text(command(json!("next")).into()))
        .await
        .unwrap();
    assert_eq!(text(&mut client).await["payload"]["data"], "next");
}

#[tokio::test]
async fn disconnect_and_revocation_cancel_inflight_handlers() {
    for revoke in [false, true] {
        let active = Arc::new(AtomicUsize::new(0));
        let mut builder = DogAppBuilder::new();
        builder.register_service("echo", Arc::new(Echo(active.clone())));
        let (tx, rx) = watch::channel(false);
        builder.set("revoke", Arc::new(rx));
        let server = server(builder.build()).await;
        let mut client = connect(&server, "a").await;
        client
            .send(Message::Text(command(json!("stall")).into()))
            .await
            .unwrap();
        wait_active(&active, 1).await;
        if revoke {
            tx.send(true).unwrap();
            assert!(matches!(message(&mut client).await, Message::Close(_)));
        } else {
            drop(client);
        }
        wait_active(&active, 0).await;
    }
}

#[tokio::test]
async fn malformed_and_oversize_frames_close_without_dispatch() {
    for payload in ["invalid".to_string(), "x".repeat(1025)] {
        let (app, active) = app(WebSocketOptions {
            max_message_bytes: Some(1024),
            ..Default::default()
        });
        let server = server(app).await;
        let mut client = connect(&server, "a").await;
        client.send(Message::Text(payload.into())).await.unwrap();
        let result = timeout(Duration::from_secs(2), client.next())
            .await
            .unwrap();
        assert!(!matches!(result, Some(Ok(Message::Text(_)))));
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn missing_pong_expires_connection() {
    let (app, _) = app(WebSocketOptions {
        heartbeat_interval_secs: Some(1),
        heartbeat_timeout_secs: Some(1),
        ..Default::default()
    });
    let server = server(app).await;
    let mut client = connect(&server, "a").await;
    assert!(matches!(message(&mut client).await, Message::Ping(_)));
    // Do not poll/flush again until the server's deadline has elapsed.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(matches!(message(&mut client).await, Message::Close(_)));
}

#[tokio::test]
async fn sse_authorization_and_gap_are_visible_over_http() {
    let (app, _) = app(Default::default());
    let server = server(app).await;
    let client = reqwest::Client::new();
    let url = format!("http://{}/private-sse", server.url);
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    let response = client
        .get(&url)
        .header("authorization", "test-only")
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let body = timeout(Duration::from_secs(2), response.text())
        .await
        .unwrap()
        .unwrap();
    assert!(body.contains("event: dogrs.stream_error"));
    assert!(body.contains("lagged"));
    assert!(!body.contains("data: 2"));
}

#[tokio::test]
async fn sse_configured_keepalive_is_applied() {
    let (app, _) = app(Default::default());
    let server = server(app).await;
    let mut response = reqwest::get(format!("http://{}/sse", server.url))
        .await
        .unwrap();
    let chunk = timeout(Duration::from_secs(2), response.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(chunk.starts_with(b":"));
}

#[tokio::test(start_paused = true)]
async fn sse_revocation_expiry_and_size_limits_end_stream() {
    for reason in ["revoked", "expired", "event_too_large", "source_closed"] {
        let (tx, rx) = broadcast::channel(8);
        let (revoker, revoke) = watch::channel(false);
        let subscription = SseSubscription {
            events: rx,
            revoke: Some(revoke),
            expires_at: None,
        };
        let mut stream = Box::pin(
            sse_stream(
                subscription,
                SseOptions {
                    max_event_bytes: Some(8),
                    max_session_secs: Some(1),
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        match reason {
            "revoked" => {
                revoker.send(true).unwrap();
            }
            "event_too_large" => {
                tx.send(json!("longer than eight bytes")).unwrap();
            }
            "source_closed" => drop(tx),
            _ => tokio::time::advance(Duration::from_secs(2)).await,
        }
        let event = stream.next().await.unwrap();
        assert_eq!(event.event, Some("dogrs.stream_error"));
        assert!(event.data.contains(reason));
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn stalled_sink_is_bounded_without_a_web_server() {
    let (app, _) = app(Default::default());
    let sink = futures_util::sink::unfold((), |_, _: Frame| async {
        std::future::pending::<Result<(), std::io::Error>>().await
    });
    let sink = Box::pin(sink);
    let input = futures_util::stream::iter([Ok::<_, std::io::Error>(Frame::Text(
        "{\"type\":\"PING\"}".into(),
    ))])
    .chain(futures_util::stream::pending());
    let end = run_websocket(
        app,
        sink,
        input,
        WebSocketOptions {
            send_timeout_secs: Some(1),
            ..Default::default()
        },
        WebSocketSession::default(),
    )
    .await;
    assert_eq!(end, SessionEnd::SlowConsumer);
}

#[tokio::test]
async fn public_broadcasts_require_opt_in_and_closed_source_terminates() {
    for enabled in [false, true] {
        let mut builder = DogAppBuilder::<Value, Params>::new();
        let (tx, _) = broadcast::channel(8);
        let tx = Arc::new(tx);
        builder.set("event_channel", tx.clone());
        if enabled {
            builder.set("ws.public_broadcasts", "true");
        }
        let server = server(builder.build()).await;
        let mut client = connect_async(format!("ws://{}/ws", server.url))
            .await
            .unwrap()
            .0;
        if enabled {
            tx.send(json!({"event":"public", "data":42})).unwrap();
            assert_eq!(text(&mut client).await["payload"], 42);
        } else {
            assert_eq!(tx.receiver_count(), 0);
            assert!(timeout(Duration::from_millis(100), client.next())
                .await
                .is_err());
        }
        client.close(None).await.unwrap();
        assert!(matches!(message(&mut client).await, Message::Close(_)));
    }
}

#[tokio::test(start_paused = true)]
async fn ws_gap_closed_source_and_expired_credentials_fail_closed() {
    for expected in [
        SessionEnd::Lagged,
        SessionEnd::SourceClosed,
        SessionEnd::Expired,
        SessionEnd::Revoked,
    ] {
        let (app, _) = app(Default::default());
        let (tx, rx) = broadcast::channel(1);
        let mut session = WebSocketSession {
            events: Some(rx),
            ..Default::default()
        };
        match expected {
            SessionEnd::Lagged => {
                tx.send(json!(1)).unwrap();
                tx.send(json!(2)).unwrap();
            }
            SessionEnd::SourceClosed => drop(tx),
            SessionEnd::Expired => session.expires_at = Some(tokio::time::Instant::now()),
            SessionEnd::Revoked => {
                let (_, rx) = watch::channel(false);
                session.revoke = Some(rx);
            }
            _ => unreachable!(),
        }
        let result = run_websocket(
            app,
            futures_util::sink::drain(),
            futures_util::stream::pending::<Result<Frame, std::io::Error>>(),
            Default::default(),
            session,
        )
        .await;
        assert_eq!(result, expected);
    }
}

#[tokio::test]
async fn concurrent_rpc_is_rejected_and_original_handler_cancelled() {
    let (app, active) = app(Default::default());
    let server = server(app).await;
    let mut client = connect(&server, "a").await;
    client
        .send(Message::Text(command(json!("stall")).into()))
        .await
        .unwrap();
    wait_active(&active, 1).await;
    client
        .send(Message::Text(command(json!("second")).into()))
        .await
        .unwrap();
    let Message::Close(Some(close)) = message(&mut client).await else {
        panic!("expected close")
    };
    assert_eq!(u16::from(close.code), 1013);
    wait_active(&active, 0).await;
}

#[tokio::test]
async fn outgoing_broadcast_limit_and_invalid_options_are_enforced() {
    let mut builder = DogAppBuilder::<Value, Params>::new();
    let (tx, _) = broadcast::channel(8);
    let tx = Arc::new(tx);
    builder.set("a", tx.clone());
    builder.set(
        "ws.options",
        Arc::new(WebSocketOptions {
            max_message_bytes: Some(64),
            ..Default::default()
        }),
    );
    let server = server(builder.build()).await;
    let mut client = connect(&server, "a").await;
    tx.send(json!({"data":"x".repeat(1000)})).unwrap();
    let Message::Close(Some(close)) = message(&mut client).await else {
        panic!("expected close")
    };
    assert_eq!(u16::from(close.code), 1009);
    assert!(WebSocketOptions {
        heartbeat_interval_secs: Some(0),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(SseOptions {
        keep_alive_interval_secs: Some(0),
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[tokio::test]
async fn connection_admission_rejects_excess_and_releases_on_disconnect() {
    let mut builder = DogAppBuilder::<Value, Params>::new();
    let limiter = Arc::new(tokio::sync::Semaphore::new(1));
    builder.set("ws.connections", limiter.clone());
    let server = server(builder.build()).await;
    let mut client = connect_async(format!("ws://{}/ws", server.url))
        .await
        .unwrap()
        .0;
    let error = connect_async(format!("ws://{}/ws", server.url))
        .await
        .unwrap_err();
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("expected HTTP rejection")
    };
    assert_eq!(response.status(), 503);
    client.close(None).await.unwrap();
    assert!(matches!(message(&mut client).await, Message::Close(_)));
    timeout(Duration::from_secs(2), async {
        while limiter.available_permits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(connect_async(format!("ws://{}/ws", server.url))
        .await
        .is_ok());
}

#[tokio::test]
async fn concurrent_clients_receive_64k_events_only_for_their_tenant() {
    let mut builder = DogAppBuilder::<Value, Params>::new();
    let (a, _) = broadcast::channel(32);
    let (b, _) = broadcast::channel(32);
    let a = Arc::new(a);
    let b = Arc::new(b);
    builder.set("a", a.clone());
    builder.set("b", b.clone());
    let server = server(builder.build()).await;
    let mut readers = tokio::task::JoinSet::new();
    for index in 0..32 {
        let tenant = if index % 2 == 0 { "a" } else { "b" };
        let mut client = connect(&server, tenant).await;
        readers.spawn(async move {
            for sequence in 0..20 {
                let event = text(&mut client).await;
                assert_eq!(event["payload"]["tenant"], tenant);
                assert_eq!(event["payload"]["sequence"], sequence);
                assert_eq!(event["payload"]["body"].as_str().unwrap().len(), 65536);
            }
        });
    }
    for sequence in 0..20 {
        for (tenant, tx) in [("a", &a), ("b", &b)] {
            tx.send(json!({"event":"update","data":{"tenant":tenant,"sequence":sequence,"body":"x".repeat(65536)}})).unwrap();
        }
        tokio::task::yield_now().await;
    }
    timeout(Duration::from_secs(10), async {
        while let Some(result) = readers.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn stalled_http_body_times_out_without_exposing_body_errors() {
    use dog_transport::{HttpOptions, IntoDogService};
    use http_body_util::{BodyExt, StreamBody};
    use tower::Service;
    let (app, active) = app(Default::default());
    let mut http = app.into_service(HttpOptions {
        request_timeout_secs: Some(1),
        ..Default::default()
    });
    let stream =
        futures_util::stream::pending::<Result<http_body::Frame<bytes::Bytes>, std::io::Error>>();
    let request = http::Request::post("/echo")
        .body(StreamBody::new(stream))
        .unwrap();
    let response = http.call(request).await.unwrap();
    assert_eq!(response.status(), 408);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    let stream = futures_util::stream::iter([Err::<http_body::Frame<bytes::Bytes>, _>(
        std::io::Error::other("secret upstream detail"),
    )]);
    let request = http::Request::post("/echo")
        .body(StreamBody::new(stream))
        .unwrap();
    let response = http.call(request).await.unwrap();
    assert_eq!(response.status(), 400);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains("secret"));
}

#[tokio::test(start_paused = true)]
async fn application_dispatch_deadlines_cancel_work_across_transports() {
    use dog_transport::{HttpOptions, IntoDogService};
    use http_body_util::{BodyExt, Full};
    use tower::Service;
    let (app, active) = app(Default::default());
    let mut http = app.clone().into_service(HttpOptions {
        request_timeout_secs: Some(1),
        ..Default::default()
    });
    let response = http
        .call(
            http::Request::post("/echo")
                .body(Full::new(bytes::Bytes::from_static(b"\"stall\"")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 408);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("timed out"));
    assert_eq!(active.load(Ordering::SeqCst), 0);
    #[cfg(any(feature = "cli", feature = "grpc"))]
    let WsPayload::Request { req } = serde_json::from_str(&command(json!("stall"))).unwrap() else {
        unreachable!()
    };
    #[cfg(feature = "cli")]
    {
        let cli = app.clone().into_service(dog_transport::CliOptions {
            request_timeout_secs: Some(1),
            ..Default::default()
        });
        let input = serde_json::to_vec(&req).unwrap();
        let mut output = Vec::new();
        cli.run(input.as_slice(), &mut output).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap()["error"]["code"],
            408
        );
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
    #[cfg(feature = "grpc")]
    {
        let grpc = app.into_service(dog_transport::GrpcOptions {
            request_timeout_secs: Some(1),
            ..Default::default()
        });
        let request = tonic::Request::new(dog_transport::grpc::proto::CallRequest {
            request_json: serde_json::to_vec(&req).unwrap(),
        });
        let error =
            dog_transport::grpc::proto::dog_transport_server::DogTransport::call(&grpc, request)
                .await
                .unwrap_err();
        assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn default_ws_requires_explicit_browser_origin_allowlist() {
    let mut builder = DogAppBuilder::<Value, Params>::new();
    builder.set(
        "ws.allowed_origins",
        Arc::new(vec!["https://trusted.example".to_string()]),
    );
    let server = server(builder.build()).await;
    for origin in ["https://trusted.example", "https://evil.example", "null"] {
        let mut request = format!("ws://{}/ws", server.url)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        let result = connect_async(request).await;
        assert_eq!(result.is_ok(), origin == "https://trusted.example");
    }
}
