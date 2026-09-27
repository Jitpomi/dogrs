# JetStream durable queue writes

The NATS adapter stores immutable binary payloads separately from revision-fenced
metadata. The portable queue API and other backends do not depend on JetStream.

For concurrent workloads on NATS 2.12 or newer, provision the dedicated KV stream
with `allow_atomic_publish = true`, then open it through its authenticated context:

```rust,no_run
# async fn example(js: async_nats::jetstream::Context) -> Result<(), Box<dyn std::error::Error>> {
let backend = dog_queue::backend::nats::NatsBackend::from_context(
    js, "jobs", 1024 * 1024, // use the smaller server/account payload limit
).await?;
# Ok(())
# }
```

`from_context` reads the stream's advertised capability; it does not modify the
stream. When atomic publishing is disabled it uses individual conditional writes.
Existing `from_store` constructors remain supported and use individual writes.
The async-nats store handle does not expose its authenticated context, so callers
must supply that context to use batching. Neither mode changes the stored format;
writers and readers using either mode can share the bucket.

The atomic writer groups only concurrent operations, with at most 32 distinct
keys and 1 MiB of value bytes per batch, two batches in flight and 256 queued
requests per backend. A quiet queue uses ordinary single-message writes. Each
caller waits for the final commit acknowledgement; staging acknowledgements never
count as success. Per-key expected revisions are checked by the server when the
whole batch commits. A known revision-conflict rejection is retried as individual
conditional writes so unrelated operations can succeed. Transport errors,
timeouts and malformed acknowledgements are **not** automatically replayed: their
commit outcome may be unknown. A batch execution is bounded to five seconds.

Local workers reserve advisory candidates while their claim is in flight. This
avoids redundant local lease races but grants no ownership. The server's exact
revision check still fences every claim; dropping/canceling the claim releases the
local hint. Other processes continue to coordinate through server revisions.

File storage, no expiry, discard-new, leader-only reads, appropriate replicas and
server fsync policy are still required. Atomic batching shares replication work;
it does not disable persistence or acknowledge an uncommitted job. Provision
replicas across appropriate failure domains independently of this library.

Protocol: [NATS ADR-50 atomic batch publishing](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-50.md).
