pub trait IntoDogService<T> {
    type Service;

    fn into_service(self, transport: T) -> Self::Service;
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HttpOptions {
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

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "http")]
pub use dog_core;
#[cfg(feature = "http")]
pub use tower;
#[cfg(feature = "http")]
pub use ::http as http_types;
#[cfg(feature = "http")]
pub use http_body_util;
#[cfg(feature = "http")]
pub use bytes;
#[cfg(feature = "http")]
pub use serde_json;
#[cfg(feature = "http")]
pub use ::tracing as tracing_lib;
#[cfg(feature = "http")]
pub use futures_util;

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
                $crate::http::DogHttpService<$crate::serde_json::Value, $params>
            >,
        ) -> impl actix_web::Responder {
            use $crate::tower::Service;

            let mut builder = $crate::http_types::Request::builder()
                .method(req.method().as_str())
                .uri(req.uri().to_string());

            for (k, v) in req.headers() {
                builder = builder.header(k.as_str(), v.as_bytes());
            }

            let http_req = builder.body($crate::http_body_util::Full::new(body)).unwrap();
            let mut service = service.get_ref().clone();
            let http_res = service.call(http_req).await.unwrap();

            let mut actix_res = actix_web::HttpResponse::build(
                actix_web::http::StatusCode::from_u16(http_res.status().as_u16()).unwrap()
            );
            for (k, v) in http_res.headers() {
                actix_res.insert_header((k.as_str(), v.as_bytes()));
            }

            let bytes = $crate::http_body_util::BodyExt::collect(http_res.into_body()).await.unwrap().to_bytes();
            actix_res.body(bytes)
        }
    };
    (poem, $fn_name:ident, $params:ty) => {
        pub fn $fn_name(
            service: $crate::http::DogHttpService<$crate::serde_json::Value, $params>
        ) -> impl poem::Endpoint {
            use poem::endpoint::TowerCompatExt;
            service.compat()
        }
    };
    (axum, $fn_name:ident, $params:ty) => {
        pub fn $fn_name(
            service: $crate::http::DogHttpService<$crate::serde_json::Value, $params>
        ) -> $crate::http::DogHttpService<$crate::serde_json::Value, $params> {
            service
        }
    };
}

#[cfg(feature = "http")]
#[macro_export]
macro_rules! declare_ws_adapter {
    (axum, $fn_name:ident, $params:ty) => {
        pub async fn $fn_name(
            ws: axum::extract::ws::WebSocketUpgrade,
            axum::extract::State(app): axum::extract::State<
                $crate::dog_core::DogApp<$crate::serde_json::Value, $params>
            >,
        ) -> impl axum::response::IntoResponse {
            ws.on_upgrade(move |socket| async move {
                use axum::extract::ws::Message as WsMessage;
                use $crate::WsPayload;
                use $crate::dog_core::DogTransportKind;
                use $crate::tracing_lib;
                use $crate::futures_util::{StreamExt, SinkExt};
                use std::sync::Arc;
                use tokio::sync::broadcast;

                let (mut ws_sender, mut ws_receiver) = socket.split();
                tracing_lib::info!("New WebSocket client connected");

                let mut broadcast_rx = app.get::<Arc<broadcast::Sender<$crate::serde_json::Value>>>("event_channel")
                    .map(|tx| tx.subscribe());

                loop {
                    let rx_fut = async {
                        if let Some(ref mut rx) = broadcast_rx {
                            rx.recv().await.ok()
                        } else {
                            tokio::time::sleep(tokio::time::Duration::from_secs(3600 * 24)).await;
                            None
                        }
                    };
                    let mut rx_fut = std::pin::pin!(rx_fut);

                    tokio::select! {
                        msg_opt = ws_receiver.next() => {
                            let msg = match msg_opt {
                                Some(Ok(m)) => m,
                                _ => break,
                            };
                            match msg {
                                WsMessage::Text(text) => {
                                    if let Ok(payload) = $crate::serde_json::from_str::<WsPayload>(&text) {
                                        match payload {
                                            WsPayload::Request { mut req } => {
                                                req.transport = DogTransportKind::WebSocket;
                                                let request_id = req.request_id.clone();
                                                tracing_lib::info!("WS Request - Service: {}, Method: {:?}", req.service, req.method);

                                                match app.handle(req).await {
                                                    Ok(res) => {
                                                        let resp = WsPayload::Response {
                                                            request_id,
                                                            payload: res.payload,
                                                            error: None,
                                                        };
                                                        if let Ok(json) = $crate::serde_json::to_string(&resp) {
                                                            let _ = ws_sender.send(WsMessage::Text(json.into())).await;
                                                        }
                                                    }
                                                    Err(err) => {
                                                        let resp = WsPayload::Response {
                                                            request_id,
                                                            payload: None,
                                                            error: Some(err.message),
                                                        };
                                                        if let Ok(json) = $crate::serde_json::to_string(&resp) {
                                                            let _ = ws_sender.send(WsMessage::Text(json.into())).await;
                                                        }
                                                    }
                                                }
                                            }
                                            WsPayload::Ping => {
                                                let resp = WsPayload::Pong;
                                                if let Ok(json) = $crate::serde_json::to_string(&resp) {
                                                    let _ = ws_sender.send(WsMessage::Text(json.into())).await;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                WsMessage::Close(_) => break,
                                _ => {}
                            }
                        }
                        event_opt = &mut rx_fut => {
                            if let Some(val) = event_opt {
                                let event_name = val.get("event").and_then(|v| v.as_str()).unwrap_or("updated").to_string();
                                let data_val = val.get("data").cloned().unwrap_or(serde_json::Value::Null);
                                let resp = WsPayload::Broadcast {
                                    event: event_name,
                                    payload: data_val,
                                };
                                if let Ok(json) = $crate::serde_json::to_string(&resp) {
                                    let _ = ws_sender.send(WsMessage::Text(json.into())).await;
                                }
                            }
                        }
                    }
                }
            })
        }
    };
}

#[cfg(feature = "http")]
#[macro_export]
macro_rules! declare_sse_adapter {
    (axum, $fn_name:ident, $channel_expr:expr) => {
        pub async fn $fn_name() -> axum::response::sse::Sse<
            impl tokio_stream::Stream<
                Item = Result<axum::response::sse::Event, std::convert::Infallible>
            > + Send + 'static
        > {
            use tokio_stream::StreamExt;
            let rx = $channel_expr.subscribe();
            let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
                .map(|msg| {
                    match msg {
                        Ok(val) => Ok(axum::response::sse::Event::default().json_data(val).unwrap()),
                        Err(e) => Err(e),
                    }
                })
                .filter_map(|r| r.ok().map(Ok::<_, std::convert::Infallible>));
            axum::response::sse::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
        }
    };
}
