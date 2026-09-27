# WebSocket and SSE operation

The `http` feature includes server-independent `realtime::run_websocket` and
`realtime::sse_stream`. They use Rust streams/sinks and bounded Tokio channels;
they do not require Axum, Redis, NATS or a database. The existing Axum macros
provide socket/frame conversion and HTTP responses. Axum is a development
dependency of this crate, not a required dependency of the shared handler.

## Authorization

WebSocket RPCs still run the application's ordinary DogApp hooks. Transport
identity is always overwritten; client-supplied internal/authenticated flags
cannot bypass these hooks. There is no automatic identity provider or tenant
policy. Configure authentication and service authorization for every application.

For private connections use:

```rust,ignore
dog_transport::declare_ws_adapter!(axum, private_ws, MyParams, authorize = authorize_ws);
```

`authorize_ws(DogApp<Value, MyParams>, axum::http::HeaderMap)` is an async function
returning `Result<realtime::WebSocketSession, axum::http::StatusCode>`. It runs
before upgrade, under the request deadline. It must verify credentials and the
browser Origin, choose an authorized tenant and event receiver, and supply:

- `tenant`: a verified `TenantContext`, replacing the tenant in every RPC.
- `headers`: verified connection headers, replacing message-supplied headers.
- `events`: an optional receiver containing only events this principal may see.
- `expires_at`: credential expiration as a Tokio `Instant`; it may shorten, never
  extend, the configured session lifetime.
- `revoke`: optional watch receiver. Sending true **or dropping its last sender**
  terminates the connection, including a running handler.

Authorization does not filter a global channel automatically. Choose scoped
receivers after verifying identity; do not derive access from a client-supplied
tenant/channel name. Authentication policy and channel storage remain application
choices. Pending bytes already handed to the network cannot be recalled.

The original three-argument WebSocket macro is suitable for public connections
or applications with per-RPC authorization hooks. It passes actual handshake
headers to every RPC, replacing headers supplied in message JSON. Browser Origins
are rejected unless they exactly match an entry in an `Arc<Vec<String>>` stored
under `ws.allowed_origins`. Opaque `null` origins are rejected. Non-browser clients
without Origin still require application authentication. Origin checking is not
authentication. Custom authorizers own their Origin policy.

Global broadcasts stay disabled unless `ws.public_broadcasts` is the string
`"true"`. The `event_channel` then contains public data only. The IoT example
explicitly allows its two local browser origins; configure deployed origins yourself.

Private SSE uses an async `authorize_sse(HeaderMap)` function returning
`Result<SseSubscription, StatusCode>`:

```rust,ignore
dog_transport::declare_sse_adapter!(
    axum, private_sse, authorize = authorize_sse,
    options = dog_transport::SseOptions::default()
);
```

Its `events`, `expires_at` and `revoke` fields have the same meaning. Authorization
has a 30-second deadline. The legacy channel-expression macro exposes that channel
as public data; middleware alone does not tenant-filter a shared channel.

## Bounds and lifecycle

| Setting | Default |
|---|---|
| WebSocket message/frame/application output limit | 1 MiB (maximum 10 MiB) |
| WebSocket in-flight RPCs per connection | 1; concurrent requests close with 1013 |
| Heartbeat interval / matching Pong deadline | 15 / 30 seconds |
| WebSocket RPC / send timeout | 30 / 5 seconds |
| Session lifetime | 1 hour; configurable up to 24 hours |
| SSE application event limit | 1 MiB (maximum 10 MiB) |
| SSE keepalive interval | 15 seconds |
| Axum macro connections | 128 per generated handler |

All configured timeouts must be 1–86,400 seconds. Invalid settings are rejected.
Store `Arc<WebSocketOptions>` as `ws.options` on the DogApp builder. Store a shared
`Arc<tokio::sync::Semaphore>` as `ws.connections` to choose a different WebSocket
limit or share one across handlers. Admission includes pending authorization;
excess clients get HTTP 503. Permits are released when the connection/body drops.
SSE macros have a fixed 128-stream limit; custom hosts can compose `sse_stream`
with their own admission policy. Hosting servers must also bound total connections,
HTTP header reads, idle body writes and TLS handshakes. The shared stream cannot
force a host to poll or close a stalled network writer.

The optional fourth argument to the public SSE macro supplies `SseOptions`.
Both option types retain their existing interval builder methods. New public
fields can be set using struct literals with `..Default::default()`.

Requests do not block heartbeat/disconnect handling. Disconnect, authorization
expiry and revocation drop the pending RPC future. Request timeout returns a
response identifying an uncertain outcome and leaves the connection usable.
Send timeout drops the connection; it does not retry a possibly partial frame.
Cancellation cannot stop synchronous blocking work or undo external side effects.
Handlers must yield and use idempotency/reconciliation when retrying mutations.

Malformed/unsupported messages close the WebSocket; oversized messages use close
code 1009. Event lag closes with 1013 and asks the client to reload a snapshot.
Source closure ends the connection. Matching protocol Pongs, not arbitrary
traffic or application JSON Pong messages, satisfy the heartbeat deadline.
Application serialization is bounded while encoding, not only after allocation.

## Delivery and resynchronization

These are ephemeral notifications, **not durable queues or replay logs**. Slow
subscribers cannot grow an unbounded per-connection buffer. SSE emits a terminal
`dogrs.stream_error` event with a reason (`lagged`, `source_closed`, `expired`,
`revoked`, or `event_too_large`) and `resync_required: true`, then ends. Control
events have a fixed small overhead outside the application payload limit.

SSE clients should close their EventSource on this event, refresh authorization
where needed, load an authoritative snapshot, then subscribe again. No event ID
or Last-Event-ID replay guarantee is fabricated. Applications requiring seamless
snapshot/replay must provide a durable cursor protocol separately. WebSocket
clients likewise reconcile after disconnect rather than assuming nothing was lost.

## Migration and evidence

Generated handlers now return `Result<impl IntoResponse, StatusCode>` and extract
HTTP headers. Normal `Router::route(..., get(handler))` registration is unchanged;
direct Rust calls to generated handlers must adapt. WebSocket clients must keep
one RPC in flight, handle protocol Ping/Pong and refresh expired sessions. Browser
clients must configure the origin allowlist or use a verified custom authorizer.
Credentials formerly embedded in WebSocket message headers must move to the
verified connection context. Public SSE and WebSocket notification streams remain
public APIs; do not route sensitive data through their global channels.

`cargo test -p dog-transport --features http,grpc,cli,iroh --all-targets --locked`
covers real WebSocket/SSE connections, authorization denial, tenant isolation,
header/transport spoofing, error sanitization, heartbeat liveness, stalled requests,
disconnect/revocation cancellation, admission/release, graceful close, frame/output
limits, gap reporting and keepalive configuration. Deterministic paused-clock tests
cover blocked sinks, expiry and body/dispatch deadlines. A concurrent test delivers
20 events with 64 KiB payloads to each of 32 clients across two tenants (640 checked
deliveries). This is a functional concurrency test, not a sustained capacity claim.
The suite also exercises HTTP, gRPC, CLI and local Iroh dispatch and deadlines.

These tests establish the listed behavior. They do not certify arbitrary network
failures, deployment capacity, or an application's authorization policy. TLS,
credential verification, reverse-proxy limits and multi-instance event distribution
remain explicit deployment responsibilities.
