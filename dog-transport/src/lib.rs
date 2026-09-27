pub trait IntoDogService<T> {
    type Service;

    fn into_service(self, transport: T) -> Self::Service;
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HttpOptions {
    pub request_timeout_secs: Option<u64>,
    pub request_id_header: Option<String>,
    pub tenant_header: Option<String>,
    pub body_limit: Option<usize>,
    pub enable_cors: Option<bool>,
    pub routes: Option<std::collections::HashMap<String, String>>,
}

impl HttpOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_id_header(mut self, header: impl Into<String>) -> Self {
        self.request_id_header = Some(header.into());
        self
    }

    pub fn tenant_header(mut self, header: impl Into<String>) -> Self {
        self.tenant_header = Some(header.into());
        self
    }

    pub fn body_limit(mut self, limit: usize) -> Self {
        self.body_limit = Some(limit);
        self
    }

    pub fn enable_cors(mut self, enable: bool) -> Self {
        self.enable_cors = Some(enable);
        self
    }

    pub fn route(mut self, path: impl Into<String>, service: impl Into<String>) -> Self {
        let mut routes = self.routes.unwrap_or_default();
        let normalized_path = path.into().trim_matches('/').to_string();
        routes.insert(normalized_path, service.into());
        self.routes = Some(routes);
        self
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GrpcOptions {
    pub request_timeout_secs: Option<u64>,
    pub enable_reflection: Option<bool>,
}

impl GrpcOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enable_reflection(mut self, enable: bool) -> Self {
        self.enable_reflection = Some(enable);
        self
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SseOptions {
    pub keep_alive_interval_secs: Option<u64>,
    pub max_event_bytes: Option<usize>,
    pub max_session_secs: Option<u64>,
}

impl SseOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keep_alive_interval_secs(mut self, secs: u64) -> Self {
        self.keep_alive_interval_secs = Some(secs);
        self
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebSocketOptions {
    pub heartbeat_interval_secs: Option<u64>,
    pub heartbeat_timeout_secs: Option<u64>,
    pub request_timeout_secs: Option<u64>,
    pub send_timeout_secs: Option<u64>,
    pub max_message_bytes: Option<usize>,
    pub max_session_secs: Option<u64>,
}

impl WebSocketOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn heartbeat_interval_secs(mut self, secs: u64) -> Self {
        self.heartbeat_interval_secs = Some(secs);
        self
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CliOptions {
    pub request_timeout_secs: Option<u64>,
    pub interactive: Option<bool>,
}

impl CliOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interactive(mut self, enable: bool) -> Self {
        self.interactive = Some(enable);
        self
    }
}

#[cfg(feature = "iroh")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IrohOptions {
    pub request_timeout_secs: Option<u64>,
    pub alpn: Vec<u8>,
    pub secret_key: Option<String>,
    pub relay_url: Option<String>,
    #[serde(skip)]
    pub endpoint: Option<iroh::Endpoint>,
}

#[cfg(feature = "iroh")]
impl IrohOptions {
    pub fn new(alpn: impl Into<Vec<u8>>) -> Self {
        Self {
            alpn: alpn.into(),
            request_timeout_secs: None,
            secret_key: None,
            relay_url: None,
            endpoint: None,
        }
    }

    pub fn secret_key(mut self, key: impl Into<String>) -> Self {
        self.secret_key = Some(key.into());
        self
    }

    pub fn relay_url(mut self, url: impl Into<String>) -> Self {
        self.relay_url = Some(url.into());
        self
    }

    pub fn endpoint(mut self, endpoint: iroh::Endpoint) -> Self {
        self.endpoint = Some(endpoint);
        self
    }
}

#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub mod realtime;
#[cfg(feature = "http")]
#[doc(hidden)]
pub use tokio;

#[cfg(feature = "iroh")]
pub mod iroh_transport;

#[cfg(feature = "iroh")]
pub mod blob_payload;

#[cfg(feature = "iroh")]
pub use blob_payload::{BlobPayloadAdapter, BlobRefPayload};

#[cfg(feature = "http")]
pub use ::http as http_types;
#[cfg(feature = "http")]
pub use ::tracing as tracing_lib;
#[cfg(feature = "http")]
pub use bytes;
#[cfg(feature = "http")]
pub use dog_core;
#[cfg(feature = "http")]
pub use futures_util;
#[cfg(feature = "http")]
pub use http_body_util;
#[cfg(feature = "http")]
pub use serde_json;
#[cfg(feature = "http")]
pub use tower;

#[cfg(feature = "http")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WsPayload {
    Request {
        #[serde(flatten)]
        req: dog_core::DogRequest,
    },
    Response {
        request_id: Option<String>,
        payload: Option<serde_json::Value>,
        error: Option<String>,
    },
    Broadcast {
        event: String,
        payload: serde_json::Value,
    },
    Ping,
    Pong,
}

#[cfg(feature = "http")]
#[macro_export]
macro_rules! declare_adapter {
    (actix, $fn_name:ident, $params:ty) => {
        pub async fn $fn_name(
            req: actix_web::HttpRequest,
            body: actix_web::web::Bytes,
            service: actix_web::web::Data<
                $crate::http::DogHttpService<$crate::serde_json::Value, $params>,
            >,
        ) -> impl actix_web::Responder {
            use $crate::tower::Service;

            let mut builder = $crate::http_types::Request::builder()
                .method(req.method().as_str())
                .uri(req.uri().to_string());

            for (k, v) in req.headers() {
                builder = builder.header(k.as_str(), v.as_bytes());
            }

            let http_req = builder
                .body($crate::http_body_util::Full::new(body))
                .unwrap();
            let mut service = service.get_ref().clone();
            let http_res = service.call(http_req).await.unwrap();

            let mut actix_res = actix_web::HttpResponse::build(
                actix_web::http::StatusCode::from_u16(http_res.status().as_u16()).unwrap(),
            );
            for (k, v) in http_res.headers() {
                actix_res.insert_header((k.as_str(), v.as_bytes()));
            }

            let bytes = $crate::http_body_util::BodyExt::collect(http_res.into_body())
                .await
                .unwrap()
                .to_bytes();
            actix_res.body(bytes)
        }
    };
    (poem, $fn_name:ident, $params:ty) => {
        pub fn $fn_name(
            service: $crate::http::DogHttpService<$crate::serde_json::Value, $params>,
        ) -> impl poem::Endpoint {
            use poem::endpoint::TowerCompatExt;
            service.compat()
        }
    };
    (axum, $fn_name:ident, $params:ty) => {
        pub fn $fn_name(
            service: $crate::http::DogHttpService<$crate::serde_json::Value, $params>,
        ) -> $crate::http::DogHttpService<$crate::serde_json::Value, $params> {
            service
        }
    };
}

#[cfg(feature = "http")]
#[macro_export]
macro_rules! declare_ws_adapter {
    (axum, $fn_name:ident, $params:ty) => {
        $crate::declare_ws_adapter!(@impl $fn_name, $params,
            |app: $crate::dog_core::DogApp<$crate::serde_json::Value, $params>, headers: axum::http::HeaderMap| async move {
                if let Some(origin) = headers.get("origin") {
                    let origin = origin.to_str().map_err(|_| axum::http::StatusCode::FORBIDDEN)?;
                    let allowed = app.get::<std::sync::Arc<Vec<String>>>("ws.allowed_origins");
                    if origin == "null" || !allowed.is_some_and(|values| values.iter().any(|value| value == origin)) {
                        return Err(axum::http::StatusCode::FORBIDDEN);
                    }
                }
                let events = if app.get::<String>("ws.public_broadcasts").as_deref() == Some("true") {
                    app.get::<std::sync::Arc<$crate::tokio::sync::broadcast::Sender<$crate::serde_json::Value>>>("event_channel").map(|tx| tx.subscribe())
                } else { None };
                Ok::<_, axum::http::StatusCode>($crate::realtime::WebSocketSession {
                    events,
                    headers: Some(headers.iter().filter_map(|(k,v)| v.to_str().ok().map(|v| (k.as_str().to_string(), $crate::serde_json::Value::String(v.to_string())))).collect()),
                    ..Default::default()
                })
            }
        );
    };
    (axum, $fn_name:ident, $params:ty, authorize = $authorize:path) => {
        $crate::declare_ws_adapter!(@impl $fn_name, $params, $authorize);
    };
    (@impl $fn_name:ident, $params:ty, $authorize:expr) => {
        pub async fn $fn_name(
            ws: axum::extract::ws::WebSocketUpgrade,
            axum::extract::State(app): axum::extract::State<$crate::dog_core::DogApp<$crate::serde_json::Value, $params>>,
            headers: axum::http::HeaderMap,
        ) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
            static CONNECTIONS: std::sync::OnceLock<std::sync::Arc<$crate::tokio::sync::Semaphore>> = std::sync::OnceLock::new();
            let limiter = app.get::<std::sync::Arc<$crate::tokio::sync::Semaphore>>("ws.connections")
                .unwrap_or_else(|| CONNECTIONS.get_or_init(|| std::sync::Arc::new($crate::tokio::sync::Semaphore::new(128))).clone());
            let permit = limiter.try_acquire_owned().map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
            let options = app.get::<std::sync::Arc<$crate::WebSocketOptions>>("ws.options").map(|o| (*o).clone()).unwrap_or_default();
            options.validate().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
            let session = $crate::tokio::time::timeout(
                std::time::Duration::from_secs(options.request_timeout_secs.unwrap_or(30)),
                ($authorize)(app.clone(), headers),
            ).await.map_err(|_| axum::http::StatusCode::REQUEST_TIMEOUT)??;
            let limit = options.message_limit();
            Ok(ws.max_message_size(limit).max_frame_size(limit)
                .write_buffer_size(0).max_write_buffer_size(limit + 1024)
                .on_upgrade(move |socket| async move {
                    let _permit = permit;
                    use $crate::futures_util::{StreamExt, SinkExt};
                    use $crate::realtime::Frame;
                    use axum::extract::ws::{Message, CloseFrame};
                    let (sink, input) = socket.split();
                    let input = input.map(|frame| frame.map(|frame| match frame {
                        Message::Text(t) => Frame::Text(t.to_string()),
                        Message::Binary(b) => Frame::Binary(b.to_vec()),
                        Message::Ping(b) => Frame::Ping(b.to_vec()),
                        Message::Pong(b) => Frame::Pong(b.to_vec()),
                        Message::Close(_) => Frame::Close(1000, "closed"),
                    }));
                    let sink = sink.with(|frame| std::future::ready(Ok::<_, axum::Error>(match frame {
                        Frame::Text(t) => Message::Text(t.into()),
                        Frame::Binary(b) => Message::Binary(b.into()),
                        Frame::Ping(b) => Message::Ping(b.into()),
                        Frame::Pong(b) => Message::Pong(b.into()),
                        Frame::Close(code, reason) => Message::Close(Some(CloseFrame { code, reason: reason.into() })),
                    })));
                    let end = $crate::realtime::run_websocket(app, sink, input, options, session).await;
                    $crate::tracing_lib::debug!(?end, "WebSocket session ended");
                }))
        }
    };
}

#[cfg(feature = "http")]
#[macro_export]
macro_rules! declare_sse_adapter {
    (axum, $fn_name:ident, authorize = $authorize:path, options = $options:expr) => {
        $crate::declare_sse_adapter!(@impl $fn_name, $options, $authorize);
    };
    (axum, $fn_name:ident, $channel_expr:expr) => {
        $crate::declare_sse_adapter!(axum, $fn_name, $channel_expr, $crate::SseOptions::default());
    };
    (axum, $fn_name:ident, $channel_expr:expr, $options:expr) => {
        $crate::declare_sse_adapter!(@impl $fn_name, $options, |_: axum::http::HeaderMap| async {
            Ok::<_, axum::http::StatusCode>($crate::realtime::SseSubscription::new($channel_expr.subscribe()))
        });
    };
    (@impl $fn_name:ident, $options:expr, $authorize:expr) => {
        pub async fn $fn_name(headers: axum::http::HeaderMap)
            -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
            static CONNECTIONS: $crate::tokio::sync::Semaphore = $crate::tokio::sync::Semaphore::const_new(128);
            let permit = CONNECTIONS.try_acquire().map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
            use $crate::futures_util::StreamExt;
            let options = $options;
            options.validate().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
            let subscription = $crate::tokio::time::timeout(std::time::Duration::from_secs(30), ($authorize)(headers))
                .await.map_err(|_| axum::http::StatusCode::REQUEST_TIMEOUT)??;
            let keep_alive = std::time::Duration::from_secs(options.keep_alive_interval_secs.unwrap_or(15));
            let stream = $crate::realtime::sse_stream(subscription, options)
                .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
                .map(move |event| {
                    let _permit = &permit;
                    let mut output = axum::response::sse::Event::default().data(event.data);
                    if let Some(name) = event.event { output = output.event(name); }
                    Ok::<_, std::convert::Infallible>(output)
                });
            Ok(axum::response::sse::Sse::new(stream)
                .keep_alive(axum::response::sse::KeepAlive::new().interval(keep_alive)))
        }
    };
}

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "grpc")]
pub mod grpc;

/// Bound application dispatch as well as transport framing. Dropping the future
/// cancels cooperative work; callers must reconcile uncertain external effects.
#[cfg(any(feature = "http", feature = "grpc", feature = "cli", feature = "iroh"))]
async fn dispatch<R, P>(
    app: &dog_core::DogApp<R, P>,
    request: dog_core::DogRequest,
    seconds: Option<u64>,
) -> Result<dog_core::DogResponse, dog_core::DogError>
where
    R: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
    P: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + Clone + 'static,
{
    let seconds = seconds.unwrap_or(30);
    if !(1..=86400).contains(&seconds) {
        return Err(dog_core::DogError::new(
            dog_core::errors::ErrorKind::GeneralError,
            "Invalid transport timeout",
        ));
    }
    tokio::time::timeout(std::time::Duration::from_secs(seconds), app.handle(request))
        .await
        .unwrap_or_else(|_| {
            Err(dog_core::DogError::timeout(
                "Request timed out; outcome may be unknown",
            ))
        })
}

/// Moving from the deprecated Axum wrapper to direct transport integration.
#[cfg(feature = "http")]
#[doc = include_str!("../MIGRATION.md")]
pub mod migration {}
