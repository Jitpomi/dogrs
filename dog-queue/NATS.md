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
Mirrored buckets retain the individual-write routing path.
`new` also uses batching when opening an existing atomic-enabled bucket.
Existing `from_store` constructors remain supported and use individual writes.
The async-nats store handle does not expose its authenticated context, so callers
must supply that context to use batching. Neither mode changes the stored format;
writers and readers using either mode can share the bucket.

Each enqueue atomically commits its immutable payload and discoverable metadata
together. A rejected idempotency-scope update leaves neither key partially written.
The atomic writer groups only concurrent operations, with at most 128 distinct
keys. Normal coalescing includes the complete logical operation that reaches or
crosses 256 KiB of value bytes, while the entire batch stays within the 2 MiB hard
bound. JetStream closes its roughly 256 KiB Raft append after the crossing entry.
An enqueue places its small metadata first and payload last within the atomic
operation; this lets four typical 64 KiB jobs finish a batch instead of stopping
at three or leaving a final metadata entry for another append. Atomic visibility
prevents discovery of metadata before its payload commits, and enqueue returns
the metadata revision for subsequent ownership checks. Larger individual logical
operations remain intact. Separate bounded execution lanes handle
enqueue pairs and metadata updates: the enqueue lane pipelines up to four
batches while the metadata lane has its own execution slot. Each input channel holds at
most 128 queued requests. Overlapping bounded acknowledgement waits avoids a
serial round-trip ceiling for small batches. Producer backlog cannot occupy the metadata lane, and lease/completion
updates never share a staging batch with large payloads. Atomic mode admits up to
128 concurrent enqueue requests to fill that bounded queue; individual-write mode
retains its 16-request default. `with_enqueue_concurrency` can override either
request limit. Request concurrency does not increase the five executing-batch limit;
the 2 MiB limit continues to bound large-payload batches. A quiet single-key update uses an ordinary write; an enqueue pair still uses one
atomic commit. Batch frames are pipelined after the server confirms staging has begun. Each
caller waits for the final commit acknowledgement; staging acknowledgements never
count as success. Per-key expected revisions are checked by the server when the
whole batch commits. A known revision-conflict rejection is retried as independent
conditional operations (enqueue pairs stay atomic) so unrelated operations can succeed. Transport errors,
timeouts and malformed acknowledgements are **not** automatically replayed: their
commit outcome may be unknown. A batch execution is bounded to five seconds. Optional `queue-diagnostics`
timings distinguish frame submission, staging acknowledgement and final durable
acknowledgement waits; only the final acknowledgement establishes success.

Local workers reserve advisory candidates while their claim is in flight. This
avoids redundant local lease races but grants no ownership. The server's exact
revision check still fences every claim; dropping/canceling the claim releases the
local hint. Other processes continue to coordinate through server revisions.

File storage, no expiry, discard-new, leader-only reads, appropriate replicas and
server fsync policy are still required. Atomic batching shares replication work;
it does not disable persistence or acknowledge an uncommitted job. Provision
replicas across appropriate failure domains independently of this library.

Protocol: [NATS ADR-50 atomic batch publishing](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-50.md).
