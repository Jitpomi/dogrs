# Dog Queue

Tenant-scoped jobs with leases, retries, cancellation and typed handlers.

## Storage and brokers

| Backend | Job state | Feature |
| --- | --- | --- |
| Memory | Process memory; lost on restart | default |
| PostgreSQL | Indexed per-job rows; optional isolated storage schemas | `postgres` |
| Redis | Atomic compare-and-swap; enable AOF and replication | `redis` |
| NATS | File-backed JetStream KV with revision checks | `nats` |
| RabbitMQ | Durable ledger plus confirmed AMQP wakeups | `rabbitmq` |
| Kafka, librdkafka client | Durable ledger plus Kafka wakeups | `kafka-rdkafka` |
| Kafka, Rust client | Durable ledger plus partition wakeups | `kafka-rskafka` |
| AWS SQS | Durable ledger plus standard/FIFO queue wakeups | `aws-sqs` |
| Google Pub/Sub | Durable ledger plus a streaming subscription | `gcp-pubsub` |

RabbitMQ, Kafka, SQS and Pub/Sub require a `JobLedger`: PostgreSQL, persistent
Redis, or JetStream. This is an intentional API change from the old prototypes,
which kept job status in process memory and could acknowledge jobs before work
finished. The broker now carries only an opaque wakeup; the ledger owns the job.

An enqueue commits to the ledger before publishing. A crash or broker outage
between those steps cannot lose the job because workers also poll the ledger.
Broker acknowledgement removes a wakeup; **only `ack_complete` completes a job**.
Notifications may be lost, duplicated, or received by a worker for another tenant.
They never grant ownership or carry customer payloads. Monitor
`notification_failures()` to detect degraded broker connectivity.

All durable ledgers support tenant-scoped active-job idempotency, scheduled jobs,
priority, cancellation, lease extension and retry recovery. They retain terminal
records; they do not implement a separate dead-letter queue or live event stream.
Poll records for status. Memory has its own in-process event stream.

## Minimal example

```rust
use dog_queue::{QueueBackend, QueueCtx, JobMessage};
use dog_queue::backend::memory::MemoryBackend;

# async fn example() -> dog_queue::QueueResult<()> {
let backend = MemoryBackend::new();
let tenant = QueueCtx::new("example");
let id = backend.enqueue(tenant.clone(),
    JobMessage::new("send-receipt", br#"{"invoice":"123"}"#.to_vec(), "json", "billing")
        .with_idempotency_key("receipt-123")
).await?;
if let Some(job) = backend.dequeue(tenant.clone(), &["billing"]).await? {
    // Perform the idempotent work before acknowledging it.
    backend.ack_complete(tenant, job.record.job_id, job.lease_token, None).await?;
}
# Ok(())
# }
```

Use `QueueAdapter` to register typed `Job` implementations and start worker pools.
Workers run the lease reaper; callers using `QueueBackend` directly must periodically
call `reclaim_expired_leases`. Configure a sensible lease duration and heartbeat
long-running work. Workers drop the handler future when renewal fails or the last
confirmed lease expires, including a stalled renewal request. Only acknowledged
renewals extend this local deadline. Backends without lease extension remain
supported: workers use their original deadline without attempting heartbeats.
Cancellation is cooperative: it cannot stop
blocking code or undo an external effect a handler has already performed. At-least-once processing requires
idempotent handlers; an idempotency key deduplicates active jobs, not all future
requests after a terminal job has been removed or completed.

## Connecting a broker

Construct the ledger, then supply a configured vendor client to the broker:

```rust,ignore
let ledger = std::sync::Arc::new(PostgresBackend::new_with_tls(config, tls).await?);
let queue = AwsSqsBackend::new(sqs_client, queue_url, ledger)?;
```

The clients retain application credential refresh, role/workload identity, TLS,
retry and endpoint configuration. No static cloud credentials are stored in Dog
configuration structs. RabbitMQ takes a `lapin::Channel` and a dedicated durable
queue name. `RdKafkaBackend` takes a producer, consumer and topic; `RsKafkaBackend`
takes a partition client. `GcpPubSubBackend` takes a publisher, subscriber and full
subscription resource name. Provision topics, subscriptions, access policies and
replication through your infrastructure tooling. These constructors do not create
billable cloud resources. Kafka's `kafka` feature enables both client adapters;
use their explicit names to choose one.

NATS `new` is a development convenience with a single file replica. Production
callers should provision a dedicated replicated KV bucket, disable direct/follower
reads, set no TTL and use discard-new, then call `from_store_with_max_payload(bucket, account_payload_limit)`.
Use the smaller of the server and account limits; hosted account limits can be
lower than the server INFO value. The older `from_store` convenience assumes a
1 MiB payload limit. `NatsConfig.subject`
now names that KV bucket, not a Core NATS subject. For NATS 2.12+ concurrent workloads,
`from_context` can use the stream’s atomic-publish capability to share durable
replication work without changing revision checks or the stored format. See
[JetStream write batching](NATS.md).

## Capacity, security and migration

PostgreSQL v2 uses indexed job rows, binary payloads and a bounded connection pool.
Prepared enqueue statements are cached per physical connection and recreated
after reconnecting. They require a direct/session connection or a pooler that
supports protocol-level prepared statements.
Redis uses one multiplexed connection manager per backend instance. The Redis
client dependency is at least 1.7.1, including the upstream fix for a duplex
read/write deadlock under backpressure (redis-rs #1955). A deterministic
request/reply pressure test guards this behavior. This fixes a transport defect,
not a universal throughput guarantee. Tenant
registration and the legacy check run once per tenant per instance. Enqueue
metadata is constructed with client time; Lua checks eligibility using Redis TIME,
and acknowledgement scripts recheck lease expiry on the server at commit.

Redis v2 uses indexed per-job metadata and separate binary payloads. JetStream
uses independent active idempotency cells, immutable payloads and separate terminal
history. Neither rewrites one growing tenant document. See [KV storage and upgrade
requirements](KV-STORAGE.md) for persistence guards, deployment assumptions,
retention, legacy data handling and recovery evidence.

Choose a ledger to match the workload; custom durable ledgers are supported through
`JobLedger`. Keep payloads bounded, use metadata snapshots for status polling, and
regularly purge terminal history. Persisted result references must serialize to at
most 4 KiB; error summaries retain at most 256 characters. Maintain provider quota
headroom for state transitions and benchmark the actual deployment.

PostgreSQL `new` uses `NoTls` for local connections or trusted tunnels;
`new_with_tls` accepts a certificate-validating connector. Redis supports `rediss`
with certificate verification. Use persistence, backups and appropriate replication
for your recovery requirements. The application must derive `QueueCtx` from a
trusted identity; accepting arbitrary tenant IDs from HTTP clients is unsafe.

PostgreSQL v2 uses `dogrs_queue_jobs_v2`, with an explicit offline v1 migration.
Redis v2 keys use a tenant hash tag; JetStream v2 separates active, payload and
history keys. Legacy Redis/JetStream tenants are rejected explicitly; drain/export
and verify migration before switching. Existing prototype data is not deleted.

SQLite/SQLx, UI and workflow placeholders are not queue implementations. They are
outside this release's broker support; enabling an unused dependency is not support.

## Verification

`cargo test -p dog-queue` runs the memory and adapter tests. Ignored integration
tests in `tests/durable_backends.rs` and `tests/broker_backends.rs` require disposable
local services. They check independent connections, exclusive leases, tenant
isolation, cancel-wins, retries, expiry, notification receipt and broker-outage
recovery. SQS and Pub/Sub are exercised against local emulators; cloud IAM, TLS
policies and provider quotas need verification in the deployment environment.

### PostgreSQL v2 upgrade

The optional PostgreSQL adapter now uses indexed per-job rows and a bounded pool.
See [storage, clock, and offline migration requirements](POSTGRES.md) before
upgrading an existing v1 ledger. Custom durable ledgers can implement the public
`JobLedger` trait; PostgreSQL is not required by the portable queue API.

### Broker notification lifecycle

Broker wrappers start two bounded background tasks lazily on enqueue/dequeue.
Publication coalesces hints (one pending plus one in flight); notification failures
never delay committed admission or ready-job claims. Workers must keep polling the
ledger, including when notifications are lost. `notification_failures()` reports
explicit receive errors and failed/timed-out publications. Idle streaming receive
deadlines are counted separately by `notification_receive_timeouts()`.

Call `shutdown_notifications(&mut self).await` to abort and join wrapper tasks
and release receive resources, including resources initialized through the direct
notification accessor. Repeated shutdown is supported; subsequent queue use
restarts tasks and subscriptions. Pub/Sub creates its stream lazily and requests
immediate redelivery of outstanding hints on shutdown. Kafka unsubscribes on
shutdown and resubscribes on receive. Client connections remain available for
restart; already queued producer requests can still finish in the SDK. Shutdown
is not a delivery flush or a guarantee that all client network I/O has stopped.

RabbitMQ waits for consumer cancellation, acknowledges its remaining prefetched
hints, and performs a channel round trip before returning. It preserves the
caller's channel and other consumers. RabbitMQ and Pub/Sub cleanup wait up to five
seconds for protocol completion and log incomplete cleanup. If RabbitMQ cleanup
fails, close the supplied channel to release outstanding deliveries before retiring
the backend. Dropping the wrapper only aborts tasks; use explicit shutdown for
receive-resource cleanup when retaining the caller's RabbitMQ channel.
Custom `Notifications` implementations must be owned (`'static`) and cancellation
safe. The notification accessor is for configuration/testing; direct receive calls
compete with background consumption once queue use has started.

RabbitMQ uses a persistent consumer with prefetch 32. SQS uses 20-second long
polls, batches up to ten receipts, and independent FIFO notification groups (job
ordering belongs to the ledger). Kafka rdkafka wakeups have no fixed key, allowing
caller-configured partitioning to distribute them. Rskafka remains an explicit
single-partition client; only offset-out-of-range errors reset its cursor.

Sharded recovery attempts every shard with concurrency eight and a 30-second
per-shard deadline. `reclaim_expired_leases_report()` returns both successful
outcomes and indexed errors. The common trait returns successful outcomes when
available, logs partial failures, and returns an error when failures occur without
any outcomes; use the report API when monitoring partial recovery directly.
