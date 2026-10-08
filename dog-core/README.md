# dog-core

DogRS service traits, hook pipelines, tenant context and in-process events.
The core requires no web server, database or async runtime. `dog-transport`
provides transport integration; `dog-axum` is a deprecated compatibility layer.

## Services

```rust
use std::sync::Arc;
use dog_core::{DogAppBuilder, DogService, TenantContext};

struct Echo;
#[async_trait::async_trait]
impl DogService<String, ()> for Echo {
    async fn create(
        &self,
        _tenant: &TenantContext,
        data: String,
        _params: (),
    ) -> anyhow::Result<String> {
        Ok(data)
    }
}

# async fn example() -> anyhow::Result<()> {
let mut builder = DogAppBuilder::<String, ()>::new();
builder.register_service("echo", Arc::new(Echo));
let app = builder.build();
let saved = app.service("echo")?
    .create(TenantContext::new("tenant-123"), "hello".into(), ())
    .await?;
assert_eq!(saved, "hello");
# Ok(())
# }
```

The service methods are `find`, `get`, `create`, `update`, `patch`, `remove` and
`custom`; unimplemented methods return errors. `ServiceHandle` runs the configured
hooks around calls. Tenant context carries a tenant identifier; it does not by
itself authorize a client or enforce filtering in a custom storage implementation.
Applications must verify identity, tenant access and data scoping.

## Events and failure monitoring

Register listeners with `DogAppBuilder::on` or `on_str`, and optionally configure
a publish filter with `publish`. Standard mutation events run after successful
service/after-hook processing. Listeners execute sequentially in the calling
future; this preserves ordering and creates no detached or unbounded task queue.

A listener error does not undo a successful mutation. Application dispatch
continues to later listeners and increments `app.event_listener_failures()`.
App clones share this counter. Poll it from application monitoring; the core
retains no error text, credentials or event payload. The count records returned
errors, not cancellation or panics. Direct `DogEventHub::emit_async` also counts
errors but returns the first error and stops, preserving its existing contract.

A slow listener delays the response and later listeners. Keep listeners short;
hand expensive work to an application-owned bounded queue with explicit overload
handling. Direct internal calls have no automatic deadline. Transport deadlines
can cancel yielding work, but cannot stop blocking code or undo external effects.
Avoid automatically retrying a mutation just because notification delivery failed.

Once listeners are selected at most once, atomically across concurrent emissions.
Selection happens before their futures run. Cancellation can therefore consume a
once listener without completing it; there is no exactly-once delivery guarantee.
Publish-filter rejection does not consume a once listener.

Events are in-process notifications, not durable delivery or cross-instance
broadcasts. Use a durable outbox/queue where delivery must survive a crash.
See `dog-transport/REALTIME.md` for WebSocket/SSE connection behavior.

## Features and companion crates

Default `json` enables JSON requests and responses. Disable default features for
format-independent services; `serde` enables serialization without JSON transport.
The optional `adapters` module supplies storage-related traits.

Companion crates include `dog-transport`, `dog-auth`, `dog-auth-local`,
`dog-auth-oauth`, `dog-typedb`, `dog-blob`, `dog-queue` and the `dog-schema` family.
Runnable applications live in `dog-examples`; follow `docs/application-structure.md`
for their layout.

Licensed under MIT OR Apache-2.0.
