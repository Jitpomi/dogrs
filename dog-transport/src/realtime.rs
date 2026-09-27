//! Bounded connection handling without a web-server dependency.
//! Subscriptions are ephemeral. Authorize the receiver before handing it to a
//! connection; after a gap clients must reload an authoritative snapshot.
use crate::{SseOptions, WebSocketOptions, WsPayload};
use dog_core::{DogApp, DogTransportKind, TenantContext};
use futures_util::{future::BoxFuture, Sink, SinkExt, Stream, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::{future::pending, io::Write, time::Duration};
use tokio::{
    sync::{broadcast, watch},
    time::{self, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(u16, &'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEnd {
    Disconnected,
    PeerClosed,
    InvalidMessage,
    TooLarge,
    Busy,
    SlowConsumer,
    HeartbeatExpired,
    Lagged,
    SourceClosed,
    Expired,
    Revoked,
    InvalidOptions,
}

/// Created by trusted application code, never deserialized from the client.
/// A receiver must contain only events the connected principal may read.
#[derive(Default)]
pub struct WebSocketSession {
    /// Set to the verified credential expiration, if earlier than the session limit.
    pub expires_at: Option<Instant>,
    pub tenant: Option<TenantContext>,
    /// When supplied, replaces all client-provided headers on every RPC.
    pub headers: Option<serde_json::Map<String, Value>>,
    pub events: Option<broadcast::Receiver<Value>>,
    /// Sending true OR dropping the last sender revokes this connection.
    pub revoke: Option<watch::Receiver<bool>>,
}

impl WebSocketOptions {
    pub fn message_limit(&self) -> usize {
        self.max_message_bytes.unwrap_or(1024 * 1024)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=10 * 1024 * 1024).contains(&self.message_limit()) {
            return Err("invalid message limit");
        }
        for n in [
            self.heartbeat_interval_secs.unwrap_or(15),
            self.heartbeat_timeout_secs.unwrap_or(30),
            self.request_timeout_secs.unwrap_or(30),
            self.send_timeout_secs.unwrap_or(5),
            self.max_session_secs.unwrap_or(3600),
        ] {
            if !(1..=86400).contains(&n) {
                return Err("timeouts must be 1..=86400 seconds");
            }
        }
        Ok(())
    }
}
impl SseOptions {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=10 * 1024 * 1024).contains(&self.max_event_bytes.unwrap_or(1024 * 1024)) {
            return Err("invalid event limit");
        }
        for n in [
            self.keep_alive_interval_secs.unwrap_or(15),
            self.max_session_secs.unwrap_or(3600),
        ] {
            if !(1..=86400).contains(&n) {
                return Err("timeouts must be 1..=86400 seconds");
            }
        }
        Ok(())
    }
}

// Bound serialization itself, not just the size check after allocation.
fn encode(value: &impl Serialize, limit: usize) -> Result<String, SessionEnd> {
    struct Limited {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Limited {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("message limit"));
            }
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Limited {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| SessionEnd::TooLarge)?;
    String::from_utf8(writer.bytes).map_err(|_| SessionEnd::InvalidMessage)
}

async fn revoked(rx: &mut Option<watch::Receiver<bool>>) {
    let Some(rx) = rx else { return pending().await };
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}
async fn event(
    rx: &mut Option<broadcast::Receiver<Value>>,
) -> Result<Value, broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => pending().await,
    }
}
async fn send<S: Sink<Frame> + Unpin>(
    sink: &mut S,
    frame: Frame,
    secs: u64,
) -> Result<(), SessionEnd> {
    match time::timeout(Duration::from_secs(secs), sink.send(frame)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(SessionEnd::Disconnected),
        Err(_) => Err(SessionEnd::SlowConsumer),
    }
}

/// One in-flight RPC per connection; no detached tasks or unbounded message queue.
/// Disconnect, expiry and revocation drop the handler future. This cannot undo
/// external effects or interrupt blocking application code.
pub async fn run_websocket<P, S, I, E>(
    app: DogApp<Value, P>,
    mut sink: S,
    mut input: I,
    options: WebSocketOptions,
    mut session: WebSocketSession,
) -> SessionEnd
where
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    S: Sink<Frame> + Unpin,
    I: Stream<Item = Result<Frame, E>> + Unpin,
{
    if options.validate().is_err() {
        return SessionEnd::InvalidOptions;
    }
    let mut revoke = session.revoke.take();
    let maximum = Instant::now() + Duration::from_secs(options.max_session_secs.unwrap_or(3600));
    let deadline = session.expires_at.unwrap_or(maximum).min(maximum);
    let end = tokio::select! {
        biased;
        _ = revoked(&mut revoke) => SessionEnd::Revoked,
        _ = time::sleep_until(deadline) => SessionEnd::Expired,
        end = websocket_loop(app, &mut sink, &mut input, &options, session) => end,
    };
    let (code, reason) = match end {
        SessionEnd::Disconnected | SessionEnd::PeerClosed => (1000, "closed"),
        SessionEnd::TooLarge => (1009, "message too large"),
        SessionEnd::InvalidMessage => (1007, "invalid message"),
        SessionEnd::Lagged => (1013, "event gap; reload snapshot"),
        SessionEnd::Busy => (1013, "one request at a time"),
        SessionEnd::Revoked => (1008, "authorization revoked"),
        SessionEnd::Expired => (1008, "session expired; reconnect"),
        SessionEnd::SourceClosed => (1001, "event source closed"),
        _ => (1001, "connection deadline"),
    };
    // A timed-out send may have left an incomplete frame. Drop that connection.
    if end == SessionEnd::PeerClosed {
        let _ = time::timeout(
            Duration::from_secs(options.send_timeout_secs.unwrap_or(5)),
            sink.close(),
        )
        .await;
    } else if end != SessionEnd::SlowConsumer && end != SessionEnd::Disconnected {
        let _ = send(
            &mut sink,
            Frame::Close(code, reason),
            options.send_timeout_secs.unwrap_or(5),
        )
        .await;
    }
    end
}

async fn websocket_loop<P, S, I, E>(
    app: DogApp<Value, P>,
    sink: &mut S,
    input: &mut I,
    options: &WebSocketOptions,
    mut session: WebSocketSession,
) -> SessionEnd
where
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    S: Sink<Frame> + Unpin,
    I: Stream<Item = Result<Frame, E>> + Unpin,
{
    let limit = options.message_limit();
    let send_secs = options.send_timeout_secs.unwrap_or(5);
    let interval = Duration::from_secs(options.heartbeat_interval_secs.unwrap_or(15));
    let mut heartbeat = time::interval_at(Instant::now() + interval, interval);
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    let mut probe: Option<(Vec<u8>, Instant)> = None;
    let mut sequence = 0u64;
    let mut request: Option<BoxFuture<'static, WsPayload>> = None;
    loop {
        let reply = tokio::select! {
            _ = async {
                match &probe {
                    Some((_, deadline)) => time::sleep_until(*deadline).await,
                    None => pending().await,
                }
            } => return SessionEnd::HeartbeatExpired,
            _ = heartbeat.tick() => {
                if probe.is_none() {
                    sequence = sequence.wrapping_add(1);
                    let token = sequence.to_be_bytes().to_vec();
                    probe = Some((token.clone(), Instant::now() + Duration::from_secs(options.heartbeat_timeout_secs.unwrap_or(30))));
                    if let Err(end) = send(sink, Frame::Ping(token), send_secs).await {
                        return end;
                    }
                }
                continue;
            }
            reply = async {
                match &mut request {
                    Some(f) => f.await,
                    None => pending().await,
                }
            } => {
                request = None;
                reply
            }
            incoming = input.next() => {
                match incoming {
                    None | Some(Err(_)) => return SessionEnd::Disconnected,
                    Some(Ok(Frame::Close(..))) => return SessionEnd::PeerClosed,
                    Some(Ok(Frame::Pong(token))) => {
                        if probe.as_ref().is_some_and(|(expected, _)| *expected == token) {
                            probe = None;
                        }
                        continue;
                    }
                    Some(Ok(Frame::Ping(token))) => {
                        if token.len() > 125 {
                            return SessionEnd::InvalidMessage;
                        }
                        if let Err(end) = send(sink, Frame::Pong(token), send_secs).await {
                            return end;
                        }
                        continue;
                    }
                    Some(Ok(Frame::Binary(_))) => return SessionEnd::InvalidMessage,
                    Some(Ok(Frame::Text(text))) => {
                        if text.len() > limit {
                            return SessionEnd::TooLarge;
                        }
                        match serde_json::from_str::<WsPayload>(&text) {
                            Ok(WsPayload::Ping) => WsPayload::Pong,
                            Ok(WsPayload::Pong) => continue,
                            Ok(WsPayload::Request { mut req }) => {
                                if request.is_some() {
                                    return SessionEnd::Busy;
                                }
                                req.transport = DogTransportKind::WebSocket;
                                if let Some(tenant) = &session.tenant {
                                    req.tenant = tenant.clone();
                                }
                                if let Some(headers) = &session.headers {
                                    req.params.inner.insert("headers".into(), Value::Object(headers.clone()));
                                }
                                let app = app.clone();
                                let seconds = options.request_timeout_secs.unwrap_or(30);
                                request = Some(Box::pin(async move {
                                    let request_id = req.request_id.clone();
                                    let result = time::timeout(Duration::from_secs(seconds), app.handle(req)).await;
                                    match result {
                                        Ok(Ok(res)) => WsPayload::Response {
                                            request_id, payload: res.payload, error: None,
                                        },
                                        Ok(Err(err)) => WsPayload::Response {
                                            request_id, payload: None,
                                            error: Some(err.sanitize_for_client().message),
                                        },
                                        Err(_) => WsPayload::Response {
                                            request_id, payload: None,
                                            error: Some("Request timed out; outcome may be unknown".into()),
                                        },
                                    }
                                }));
                                continue;
                            }
                            _ => return SessionEnd::InvalidMessage,
                        }
                    }
                }
            }
            value = event(&mut session.events) => {
                match value {
                    Err(broadcast::error::RecvError::Lagged(_)) => return SessionEnd::Lagged,
                    Err(broadcast::error::RecvError::Closed) => return SessionEnd::SourceClosed,
                    Ok(val) => WsPayload::Broadcast {
                        event: val.get("event").and_then(Value::as_str).unwrap_or("updated").to_string(),
                        payload: val.get("data").cloned().unwrap_or(Value::Null),
                    },
                }
            }
        };
        let text = match encode(&reply, limit) {
            Ok(text) => text,
            Err(end) => return end,
        };
        if let Err(end) = send(sink, Frame::Text(text), send_secs).await {
            return end;
        }
    }
}

/// A receiver chosen by application authorization, with optional live revocation.
pub struct SseSubscription {
    pub events: broadcast::Receiver<Value>,
    pub revoke: Option<watch::Receiver<bool>>,
    pub expires_at: Option<Instant>,
}
impl SseSubscription {
    pub fn new(events: broadcast::Receiver<Value>) -> Self {
        Self {
            events,
            revoke: None,
            expires_at: None,
        }
    }
}
pub struct SseEvent {
    pub event: Option<&'static str>,
    pub data: String,
}

/// Emits a terminal `dogrs.stream_error` on a gap/expiry, then closes. No replay
/// cursor is fabricated: reconnecting clients must reconcile their state.
pub fn sse_stream(
    subscription: SseSubscription,
    options: SseOptions,
) -> Result<impl Stream<Item = SseEvent> + Send, &'static str> {
    options.validate()?;
    let maximum = Instant::now() + Duration::from_secs(options.max_session_secs.unwrap_or(3600));
    let deadline = subscription.expires_at.unwrap_or(maximum).min(maximum);
    let limit = options.max_event_bytes.unwrap_or(1024 * 1024);
    Ok(futures_util::stream::unfold(
        (subscription, false),
        move |(mut sub, done)| async move {
            if done {
                return None;
            }
            let result = tokio::select! {
                biased;
                _ = revoked(&mut sub.revoke) => Err("revoked"),
                _ = time::sleep_until(deadline) => Err("expired"),
                event = sub.events.recv() => match event {
                    Ok(value) => encode(&value, limit).map_err(|_| "event_too_large"),
                    Err(broadcast::error::RecvError::Lagged(_)) => Err("lagged"),
                    Err(broadcast::error::RecvError::Closed) => Err("source_closed"),
                }
            };
            let (event, done) = match result {
                Ok(data) => (SseEvent { event: None, data }, false),
                Err(reason) => (
                    SseEvent {
                        event: Some("dogrs.stream_error"),
                        data: json!({"reason": reason, "resync_required": true}).to_string(),
                    },
                    true,
                ),
            };
            Some((event, (sub, done)))
        },
    ))
}
