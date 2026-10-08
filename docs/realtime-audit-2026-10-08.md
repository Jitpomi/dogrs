# Initial realtime audit — October 8, 2026

Reviewed the tree at `f536e52` after merging queue PR #24. There is no separate
`dog-realtime` crate: shared WebSocket/SSE logic lives in
`dog-transport/src/realtime.rs`, host adapters in `dog-transport/src/lib.rs`, and
service event dispatch in `dog-core/src/events.rs` and `dog-core/src/app.rs`.
The queue capacity default remains 900 jobs/second. Queue throughput results do
not establish realtime throughput.

## Verified baseline

`cargo test -p dog-transport --features http --test realtime --locked` passed
19 tests. They cover real WebSocket/SSE authorization, tenant separation, trusted
header/transport replacement, connection admission/release, heartbeats, request
cancellation, size limits, slow sinks, revocation, expiry and visible event gaps.
The concurrency case checks 640 deliveries (32 clients × 20 events) with 64 KiB
payloads across two tenants. It is not a sustained-load or soak test.

`cargo test -p dog-core --locked` passed 14 unit tests and one doctest; five
other doctests are ignored. Those unit tests cover errors, not the event hub.
They should not be presented as event-dispatch correctness evidence.

## Findings requiring follow-up

1. **Listener failures are silent in application dispatch.** Both
   `DogApp::emit_custom` and the standard-event path in `ServiceHandle::run_pipeline`
   discard listener errors with `let _ = ...await`. A failed notification can
   therefore be invisible to operators while the service returns success.
   Add an observable failure path and regression tests without converting a
   successful service mutation into a retry-inducing failure. Avoid logging raw
   payloads or credential-bearing error details.

2. **Listeners execute serially in the request future.** A pending listener
   prevents later listeners and delays the response after the service operation.
   Transport deadlines bound yielding async work, but cannot undo committed
   effects, interrupt blocking code or provide a deadline for direct internal
   calls. Document this contract and test pending/error/cancellation behavior.
   Applications should hand off expensive work to a bounded mechanism; do not
   introduce unlimited detached tasks or silently change event ordering.

3. **Event-hub concurrency and failure contracts need focused tests.** The core
   suite does not currently exercise concurrent once listeners, publish filtering,
   listener failures or cancellation during delivery. The atomic once flag is
   claimed during snapshot creation, before its future is executed. A canceled
   dispatch therefore need not run a once listener. Specify and test at-most-once
   selection rather than promising exactly-once successful execution.

These are findings from the inspected code, not a new claim of tenant leakage or
observed data loss. No realtime implementation was changed during this audit.

## Existing boundaries to retain

Broadcast receivers are ephemeral. The transport closes on lag rather than
silently continuing after dropped events, consistent with
[Tokio broadcast lag semantics](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html).
Receivers must already be authorized and appropriately scoped by the application.
A reconnect requires reconciliation; no durable cursor/replay protocol exists.

The SSE adapter uses the host's stream and keepalive facilities, as described by
[Axum's SSE API](https://docs.rs/axum/latest/axum/response/sse/index.html). A shared
stream cannot force a host to poll or close a stalled network writer. Host-level
write deadlines, TLS, authorization policy and multi-instance distribution remain
application/deployment responsibilities, documented in `dog-transport/REALTIME.md`.

Next work should address event failure visibility and add the focused core tests
before selecting a measured realtime concurrency/latency workload. No sustained
realtime capacity target has been agreed or validated in this audit.
