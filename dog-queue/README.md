# Dog Queue

Tenant-scoped jobs with leases, retries, cancellation and typed handlers.

## Storage and brokers

| Backend | Job state | Feature |
| --- | --- | --- |
| Memory | Process memory; lost on restart | default |
| PostgreSQL | Transactional, versioned tenant records | `postgres` |
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
long-running work. Cancellation prevents later completion, but cannot undo a side
effect a handler has already performed. At-least-once processing requires
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
now names that KV bucket, not a Core NATS subject.

## Capacity, security and migration

PostgreSQL v2 uses indexed job rows, binary payloads and a bounded connection pool.
Redis and JetStream currently serialize tenant state and target modest job volumes.
Choose a ledger to match the workload; custom durable ledgers are supported through
`JobLedger`. Keep payloads bounded, use `get_snapshot` for metadata polling, and regularly use
`purge_terminal_before(ctx, cutoff)` on the ledger. JetStream's maximum value and
server message limits also bound tenant state. NATS admission reserves 8 KiB per
record for later status updates within a state budget capped at 900 KB and reduced for smaller account/stream
payload limits (with 4 KiB reserved for protocol framing). Purge terminal
history before that budget fills. Persisted result references must serialize to
at most 4 KiB; error summaries retain at most 256 characters. Benchmark realistic tenant volume
and maintain free capacity for status updates before deploying.

PostgreSQL `new` uses `NoTls` for local connections or trusted tunnels;
`new_with_tls` accepts a certificate-validating connector. Redis supports `rediss`
with certificate verification. Use persistence, backups and appropriate replication
for your recovery requirements. The application must derive `QueueCtx` from a
trusted identity; accepting arbitrary tenant IDs from HTTP clients is unsafe.

PostgreSQL uses `dogrs_queue_state_v1`; Redis uses `{dogrs-queue-v1}:*`. Existing
prototype tables/keys/messages are neither imported nor deleted. Drain/export and
verify a migration before switching. Broker constructors and OAuth/transport API
changes are described in the repository release notes.

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
