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
enqueue pairs and metadata updates: the enqueue lane defaults to two
batches while the metadata lane has its own execution slot. Each input channel holds at
most 128 queued requests. Overlapping bounded acknowledgement waits avoids a
serial round-trip ceiling for small batches. Producer backlog cannot occupy the metadata lane, and lease/completion
updates never share a staging batch with large payloads. Atomic mode admits up to
128 concurrent enqueue requests to fill that bounded queue; individual-write mode
retains its 16-request default. `with_enqueue_concurrency` can override either
request limit. Request concurrency does not increase the configured executing-batch limit;
the 2 MiB limit continues to bound large-payload batches. A quiet single-key update uses an ordinary write; an enqueue pair still uses one
atomic commit. Atomic frames are pipelined in connection order without waiting for staging.
Every frame requires API level 2, and the bounded writer is enabled only when the
stream advertises atomic publishing. This is the atomic protocol, not fast ingest
with a negotiated flow window. Each caller waits for a validated final commit
acknowledgement; empty staging replies never count as success. Per-key expected revisions are checked by the server when the
whole batch commits. A known revision-conflict rejection is split into smaller conditional batches
until the conflicting logical operations are isolated; enqueue pairs stay atomic.
Unaffected operations remain batched instead of falling back to individual writes.
Only one child commit runs at a time within the original execution slot. A single
conflict in 64 operations therefore requires at most 13 attempts rather than 65;
if every operation conflicts, the bounded worst case is 127 attempts. Transport errors,
timeouts and malformed acknowledgements are **not** automatically replayed: their
commit outcome may be unknown. A batch execution, including all conflict splitting, is bounded to five seconds.
If a later child times out, earlier confirmed child outcomes are preserved. The
in-flight child is uncertain; children not yet attempted are reported separately
and are not submitted after the deadline. Optional `queue-diagnostics`
timings distinguish frame submission and final durable acknowledgement waiting;
only the final acknowledgement establishes success.

Pooled subscriptions use a distinct reply subject for every batch. Replies from
an earlier batch are ignored before inspecting their status or payload, including
errors and empty staging replies that have no batch ID. A delayed conflict reply
therefore cannot reject a later batch using the same subscription.
For the standard publish path, only the first and final frames request replies;
intermediate staging replies are optional under
[ADR-50](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-50.md).
Custom JetStream API-prefix routing retains its existing per-frame request path.
Both paths require the same validated final commit acknowledgement. This reduces
reply traffic on the standard path, not the number of durable writes. Diagnostic
staging intervals consequently observe fewer replies and must not be compared
directly with the earlier per-frame-reply measurements.

Local workers reserve advisory candidates while their claim is in flight. This
avoids redundant local lease races but grants no ownership. The server's exact
revision check still fences every claim; dropping/canceling the claim releases the
local hint. Other processes continue to coordinate through server revisions.

Cloning an `async_nats::jetstream::Context` shares its underlying connection.
When constructing multiple sharded backends, callers can supply independently
configured contexts to avoid putting all stores' payloads and control traffic
through one connection. `NatsBackend::new` already creates a connection per
backend. `from_context` preserves the caller's authentication and TLS choices;
it does not attempt to reconstruct credentials or silently create connections.
The capacity fixture can compare `DOGRS_NATS_CONNECTIONS=shared` and `per-shard`
on one runner, with the same workload, replica count and fsync policy.

File storage, no expiry, discard-new, leader-only reads, appropriate replicas and
server fsync policy are still required. Atomic batching shares replication work;
it does not disable persistence or acknowledge an uncommitted job. Provision
replicas across appropriate failure domains independently of this library.

Protocol: [NATS ADR-50 atomic batch publishing](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-50.md).

### Overlapping claim and payload reads

The adapter reads a candidate's immutable payload while its conditional metadata
claim is in flight. It returns the job only after that exact revision is durably
claimed. A lost or uncertain claim cannot return prefetched bytes. A failed early
read receives one fresh read after a successful claim; uncertain writes are never
replayed. Both the watch-hint path and the point-read fallback use this rule.
Speculative reads, including payloads waiting for ownership, are bounded to 128
per store. Excess consumers use claim-then-read without waiting for a prefetch
permit. Producer admission remains independent. This removes a serial network wait without caching payloads,
changing the stored format, or assuming that producers and consumers share a process.

### Small-batch collection

The atomic writer allows up to 1 ms of collection for enqueue pairs and up to
8 ms for metadata updates, so independently arriving operations can share a
durable commit. Collection stops early at the batch bounds or a conflicting key.
A quiet metadata update can therefore incur the full 8 ms collection delay. Byte/message bounds, the separate metadata lane, expected-revision
checks and final durable-acknowledgement requirements are unchanged. Increasing
producer concurrency is not a substitute for measuring durable storage latency;
it can increase timeouts. Consumer concurrency must also cover the measured
claim-plus-completion latency at the required arrival rate. As a planning
estimate, required workers are arrival rate multiplied by mean claim, handler,
and completion cycle time, plus headroom. At ten jobs/second, two workers can
sustain only about 200 ms of mean total cycle time. Increasing this application
worker count is different from increasing the writer's batch execution limits.
See the [concurrency experiments](../docs/jetstream-batching-experiment-b51fb4f.md)
for measured improvements and the still-unmet full capacity target.

### Execution limits

`DOGRS_NATS_ENQUEUE_CONCURRENCY` (default 2) and
`DOGRS_NATS_UPDATE_CONCURRENCY` (default 1) accept integers from 1 through 32.
Invalid values fail atomic-writer construction before tasks start. Limits are per
store, not a global limit across shards. These execution limits differ from
`with_enqueue_concurrency`, which bounds admission requests.

Revision fencing does not implement a server-time expiry predicate. Lease
transitions use application wall clocks before CAS; clock skew and delayed commits
remain deployment/contract limitations described in `KV-STORAGE.md`. Do not infer
arbitrary-clock-skew safety from a successful normal recovery test.

### Lease response deadlines and stream validation

Existing buckets must use `Limits` retention. `Interest` and `WorkQueue` retention
are rejected because job records and payloads must survive independently of
consumer interest and acknowledgement.

Completion, failure and heartbeat writes are locally bounded by the previously
observed lease deadline. A deadline already passed prevents submission. A timeout
or successful acknowledgement observed after that deadline returns an uncertain
outcome error, not success: the remote write may have committed. Reconcile job
status before deciding whether to retry; do not assume the write was rolled back.
A dequeue also rechecks expiry after both ownership and payload retrieval finish,
so a payload delay cannot knowingly hand an already expired lease to a worker.
The stored lease is left for normal expiry recovery; it is not rolled back over a
potential concurrent owner.

These guards do not add a server-side time predicate to JetStream CAS. An in-flight
write can still commit after its application deadline, and clock synchronization
remains required. Strict server-time commit expiry needs a different coordination
mechanism; revision fencing alone cannot provide it. The guards do not establish
an increased capacity limit or remove the cost of KV metadata transitions.

### Size bucket count to measured storage capacity

More sharded buckets are not automatically faster: each is an independent
replicated stream. In a local three-replica, always-fsync comparison, four buckets
with the pre-window-change writer completed 45,586–48,413 jobs, versus 19,264–24,652 with
sixteen, under the same 60,000-job workload. Neither met the full gate. Increasing
the soft enqueue batch target to 1 MiB made completion throughput worse and was
reverted. See [experiment details](../docs/jetstream-batching-experiment-b51fb4f.md).
These are deployment-sizing observations, not a universal default or capacity
promise. Keep shard topology stable for existing data; changing it requires migration.

The metadata-window follow-up in the linked report improved completion counts
but still missed the 1,000 jobs/second target. Collection changes do not remove a
job's durable state transitions or establish a capacity guarantee.

### Admission attribution

[Direct-write controls and server traces](../docs/jetstream-admission-attribution-3d1dbd3.md)
locate significant waiting in JetStream's Raft WAL fsync path. Opt-in queue
metrics now distinguish observed first-staging and post-staging acknowledgement
intervals; neither is an isolated disk measurement. Native writes also missed the
local admission target. This evidence does not certify 1,000 jobs/second, justify
weaker persistence, or rule out further adapter improvements.

The [matched payload-size comparison](../docs/jetstream-write-cost-c472a04.md)
cross-checks acknowledged records against all replicas' stream sequences. In those
runs, both payload sizes wrote four logical records per completed job, but 1 KiB
passed the job-rate target while 64 KiB missed it with much longer commit waits.
A [separate payload-storage prototype](../docs/jetstream-split-storage-experiment.md)
was tested and rejected: it missed the full target and introduced uncertain
enqueue timeouts in one run. The existing storage layout remains unchanged.
