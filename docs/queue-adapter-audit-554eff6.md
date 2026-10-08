# Dog-queue adapter audit

Reviewed commit: `554eff65226e2d9e182713a6e686dd13f1e287e1` (`dog-queue` 0.2.0).
Review date: October 7, 2026 (America/Denver).

This is a source and official-documentation audit, not a new certification or a
claim that every possible defect has been excluded. Findings marked confirmed
are established by control flow or configuration in this commit; they were not
reproduced in new fault-injection tests during this audit. Performance candidates
need controlled measurements before being called the cause of a capacity miss.
No implementation changes were made.

## Evidence and scope

The fresh [correctness/recovery run, attempt 2](https://github.com/Jitpomi/dogrs/actions/runs/37620510981/attempts/2)
passed all 11 jobs. Recovery includes PostgreSQL and Redis process restart,
JetStream leader loss, and Redis restoration after a 300-second outage.

Fresh capacity runs used 100 tenants, 10 jobs/second/tenant, unique 64 KiB payloads,
60 seconds of offers and five seconds of drain, without diagnostic profiling.

| Backend | Accepted | Completed at deadline | Overload | Operation errors | Gate |
|---|---:|---:|---:|---:|---|
| [PostgreSQL](https://github.com/Jitpomi/dogrs/actions/runs/37706802406) | 45,877 | 45,877 | 14,123 | 0 | Fail |
| [Redis](https://github.com/Jitpomi/dogrs/actions/runs/37706805365) | 60,000 | 60,000 | 0 | 0 | Pass |
| [JetStream](https://github.com/Jitpomi/dogrs/actions/runs/37706808251) | 57,393 | 26,839 | 2,607 | 0 | Fail |

Both PostgreSQL and Redis verified every accepted job. JetStream did not finish
the accepted workload by the deadline. These are separate shared runners, not a
controlled hardware comparison. Redis and PostgreSQL use single-server fixtures;
JetStream uses three replicas on one runner. Durability settings remain enabled,
but the replication topologies are not equivalent. No infrastructure-only cause
follows from these results.

## Architecture

Redis, PostgreSQL and JetStream implement authoritative job ledgers. RabbitMQ,
SQS, Pub/Sub and both Kafka clients implement notifications around a supplied
`Arc<dyn JobLedger>`. Their notifications contain only `dogrs-wakeup-v1`.
Acknowledging a notification does not complete a job. This is an intentional
ledger-plus-notification architecture, not native broker job processing.

The common API remains provider-independent. Each optional adapter nevertheless
has provider-specific implementation costs and deployment assumptions. The recent
capacity runs exercise the ledgers directly, not `BrokerBackend`.

## Prioritized confirmed findings

### P1: JetStream dispatcher accepts zero execution concurrency

[`nats_batch.rs:102`](https://github.com/Jitpomi/dogrs/blob/554eff65226e2d9e182713a6e686dd13f1e287e1/dog-queue/src/backend/nats_batch.rs#L102)
parses `DOGRS_NATS_ENQUEUE_CONCURRENCY` and `DOGRS_NATS_UPDATE_CONCURRENCY` as
numbers without validating their range. With zero, `running.len() >= concurrency`
is always true, even for an empty JoinSet; `join_next()` immediately returns None.
The dispatcher spins instead of receiving requests. Oversized values also remove
the intended practical limit on executing batches.

Fix: validated, explicit options with positive upper-bounded values; reject
invalid configuration before spawning tasks. Test zero, overflow, invalid text,
maximum values, shutdown and timeouts. This is a code defect, but the capacity
fixture uses defaults, so it does not explain that run's failure.

### P2: notifications delay ready ledger work

[`broker.rs:80`](https://github.com/Jitpomi/dogrs/blob/554eff65226e2d9e182713a6e686dd13f1e287e1/dog-queue/src/backend/broker.rs#L80)
waits up to 50 ms on `Notifications::receive()` before asking the ledger for a job,
even with an existing backlog. Enqueue also waits up to two seconds for an optional
notification after the authoritative job has committed. Silent receive timeouts
are not included in the receive-failure counter (unlike explicit errors).

Fix: drain notifications in a bounded background task or use a ledger-first
strategy with safe notification registration and periodic fallback polling.
Coalesce wakeups and bound publication separately from durable admission. Count
normal empty polls separately from transport timeouts; preserve observability.
Test a backlogged ledger with notifications pending forever, publication failure,
lost wakeups, cancellation and shutdown. This affects broker wrappers, not the
direct Redis/PostgreSQL/JetStream capacity runs.

### P2: RabbitMQ uses discouraged per-message polling

[`rabbitmq.rs:73`](https://github.com/Jitpomi/dogrs/blob/554eff65226e2d9e182713a6e686dd13f1e287e1/dog-queue/src/backend/rabbitmq.rs#L73)
uses `basic_get` for each dequeue. RabbitMQ explicitly recommends long-lived
consumers instead of this polling approach. Replace it with a persistent consumer,
bounded prefetch and a notification buffer integrated with the shared wrapper fix.
Preserve publisher confirms, mandatory routing and durable-ledger fallback.
[Official consumer guidance](https://www.rabbitmq.com/docs/consumers#polling).

### P2: published tuning documentation differs from implementation

`dog-queue/README.md:102` describes separate producer/control Redis connections
and pipelined registration/server time. `redis.rs` has one ConnectionManager,
cached legacy/registration checks, and client time during enqueue; the Lua write
still uses Redis time for eligibility and lease checks.

`dog-queue/NATS.md` describes four executing enqueue batches and a 2 ms window.
Current code defaults to two enqueue batches, one metadata batch and a conditional
1 ms collection window. Align documentation with executable defaults and expose
effective settings in measurement reports. Do not silently restore older code
merely to make the documentation true.

## Redis ledger

**Appropriate choices:** separate binary payload and metadata hashes; sorted-set
indexes; tenant hash tags; exact-record CAS; server-time lease-expiry validation
in `redis_write.lua`; bounded contention retries; optional startup persistence
guard. Terminal history does not enter normal candidate selection.

**Performance candidates:** claim and completion read metadata, apply the common
state machine in Rust, then conditionally write. Competing workers can read the
same queue head and retry. All traffic shares one connection manager. These are
real implementation costs, not proof of a defect or the cause of any prior miss.
Lua executes atomically but blocks other server activity while running; keep
promotion and purge work bounded. [Redis scripting](https://redis.io/docs/latest/develop/programmability/eval-intro/).

**Durability boundary:** the guard checks AOF, `appendfsync=always` and no eviction;
it does not wait for replica durability or validate failover guarantees. Redis
replication is asynchronous by default, and even WAIT is not a general strong
consistency guarantee. A single-node capacity pass cannot certify lossless
failover. [Persistence](https://redis.io/docs/latest/management/persistence/),
[replication](https://redis.io/docs/latest/operate/oss_and_stack/management/replication/).

**Next checks:** contention with many workers on one tenant, large delayed-job
sets, sustained AOF rewrite overlap, partial loss/restore of the tenant registry,
and explicitly selected replica-persistence policy if stronger HA is promised.
Do not replace the current server-side checks with weaker client checks for speed.

## PostgreSQL ledger

**Appropriate choices:** per-job rows, binary payloads, partial indexes, bounded
pool and producer admission, atomic enqueue, `FOR UPDATE SKIP LOCKED`, server
timestamps, token fencing, and batched claims/completions. PostgreSQL documents
SKIP LOCKED as useful for queue-like access, despite being unsuitable for general
consistent views. [SELECT](https://www.postgresql.org/docs/current/sql-select.html).

**Performance candidates, not established root causes:** each lifecycle updates
JSONB state and indexed status/lease/active fields. Index-changing updates limit
HOT opportunities and create tuple/index churn. Claim ordering and eligible-time
filtering use different index orders; many future-dated or high-priority jobs can
change scan cost. The claim batch stops at a repeated tenant, so batch occupancy
depends on arrival order. Pool and batch-slot waits can amplify storage latency.
[HOT](https://www.postgresql.org/docs/current/storage-hot.html),
[VACUUM](https://www.postgresql.org/docs/current/sql-vacuum.html).

**Next checks:** capture actual batch sizes, pool waits, SQL time, WAL bytes,
checkpoint activity, dead tuples, and EXPLAIN (ANALYZE, BUFFERS, WAL) for the exact
queries with realistic ready/delayed/terminal distributions. Compare designs only
after locating dominant time; splitting immutable payload storage or changing
indexes may help but is not yet justified as the fix.

**Durability boundary:** server settings determine commit persistence and standby
acknowledgements; the adapter does not force synchronous replication. Caller-
supplied TLS is supported; `new` uses NoTls and should not be mistaken for a secure
remote deployment default. [WAL settings](https://www.postgresql.org/docs/current/runtime-config-wal.html).

No new correctness defect in the PostgreSQL hot paths was established by this
review. One pass and one miss on different commits/runners do not isolate a cause.

## JetStream ledger

**Appropriate choices:** exact-revision CAS, leader reads, separate immutable
payloads, bounded admission, and waiting for the final atomic-publish commit
acknowledgement. Discovery hints do not grant ownership. Official KV documentation
explains that KV sits on streams; this is a valid storage primitive, not inherently
an incorrect use of NATS. [KV documentation](https://docs.nats.io/learn/key-value/),
[atomic publishing ADR-50](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-50.md).

**Concrete design cost:** this is a KV job-state machine, not a native durable
work-queue consumer. In atomic mode a fresh enqueue writes payload plus metadata;
claim CAS writes metadata and reads the payload; completion CAS writes metadata.
Watch delivery adds discovery traffic. The metadata execution lane defaults to
one batch per store. Candidate selection scans the tenant's active metadata.
These costs are attributable to DogRS code, although their share of the observed
delay still requires measurement. Native stream delivery is a useful lower-bound
comparison, but cannot replace this API without preserving cancellation, priority,
scheduling, status lookup, deduplication and lease-token semantics.

**Known lease limitation:** `nats_records.rs:844-1044` applies transitions with
`Utc::now()` before sending CAS. CAS verifies a revision, not a timestamp predicate
at server commit. A slow clock or a delayed write can therefore accept an operation
after the intended wall-clock deadline if no intervening revision has replaced
it. CAS still rejects a replaced owner. `KV-STORAGE.md` already documents clock
synchronization requirements; this is not a newly observed data-loss incident.
Test delayed commits and skew explicitly and define the supported lease contract.
Do not claim Redis/PostgreSQL-style server-time expiry enforcement for this path.

**Other limits:** individual-write mode can leave a payload orphan if execution
stops after payload persistence and before metadata CAS. The documentation
acknowledges this; a safe reconciliation tool needs to distinguish abandoned data
from uncertain in-flight commits. Startup watch replay includes active-cell
subjects, which can still contain terminal records on disk even though the local
index discards them. Retained history therefore warrants startup/reconnect tests.

**Next performance test:** on the same pinned deployment compare native durable
publishes, the exact KV operation sequence, and the full adapter; keep three
replicas, fsync policy, payload distribution and connections identical. Measure
metadata-batch occupancy/wait, durable acknowledgement, payload fetch, discovery
and conflict rate separately. This can distinguish provider cost from adapter cost.

## Notification adapters, individually

| Adapter | Sound behavior | Improvement / boundary |
|---|---|---|
| RabbitMQ | Confirmed, mandatory, persistent wakeups; ledger owns jobs | Replace basic_get polling; exercise channel recovery. Quorum-queue provisioning is an operator concern. |
| SQS | Caller SDK client; deleting a wakeup cannot delete the ledger job | Hard-coded short polling, one receive/delete at a time, and one FIFO group constrain efficiency. Move long polling/batched deletes to a background receiver; do not just extend the shared 50 ms wait. |
| Kafka / rdkafka | Caller security/configuration; offsets acknowledge wakeups only | Constant key `dogrs` ordinarily concentrates hints on one partition under key-based partitioning. Permit configurable partition distribution. Async offset commit errors are not fully represented by the wrapper's receive counter. |
| Kafka / rskafka | Explicit partition client; lost/repeated hints are safe | Deliberately one partition and a mutex-protected offset. Every fetch error attempts an offset reset to latest, not only retention errors. Narrow classification and record recovery; generalize partitions if notification throughput requires it. |
| Pub/Sub | Persistent StreamingPull session and explicit outstanding-message/byte limits | Shared mutex receive is ahead of ledger access; a background consumer can preserve flow control while removing that dependency. Test stream restart and acknowledgement failure observability with the SDK. |

AWS documents why long polling reduces empty receives;
[SQS polling](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-short-and-long-polling.html).
Kafka clients expose partitioning and batching controls;
[Kafka configuration](https://kafka.apache.org/41/configuration/producer-configs/),
[librdkafka configuration](https://github.com/confluentinc/librdkafka/blob/master/CONFIGURATION.md).
Google recommends high-level streaming subscribers and flow control;
[Pub/Sub guidance](https://docs.cloud.google.com/pubsub/docs/subscribe-best-practices).

These notification improvements cannot explain the direct JetStream capacity
miss. Cloud IAM, TLS, quotas and regional failures also are not established by
the latest emulator-backed SQS/Pub/Sub CI jobs.

## Memory and sharding

Memory uses process-local locks and data; it is intentionally not a persistent
ledger. Priority insertion and lease reaping have linear work, and retained job
history consumes memory. It is appropriate for its documented development scope,
not restart durability. This review did not repeat its full concurrency audit.

ShardedBackend uses stable tenant hashing. Changing shard order/count changes
placement and requires migration, as documented. Reaping visits shards serially
and returns on the first error; a persistently failing early shard can prevent
later healthy shards from reclaiming leases. Improve fault isolation with bounded
concurrent/all-shard attempts and a way to retain successful outcomes alongside
errors. Add a failing-first-shard regression test.

## Recommended order of work

1. Validate JetStream execution limits and fix inaccurate tuning documentation.
2. Remove notification waits from ready-job processing; use provider-native
   background consumers, with shutdown and failure tests.
3. Isolate shard reaping failures and qualify JetStream lease-clock semantics.
4. Measure PostgreSQL and JetStream at the operation level on pinned infrastructure.
5. Change the measured bottleneck, repeat the same workload, and retain both
   successful and unsuccessful evidence.

The audit does not support calling JetStream naturally slow or declaring DogRS's
provider-independent API wrong. It identifies specific adapter issues and precise
remaining measurement work, while preserving the passing correctness evidence.

## Follow-up implementation

The `codex/queue-adapter-audit-fixes` patch addresses the confirmed execution-limit,
notification-path, RabbitMQ polling, sharded recovery, and documentation findings.
It also adds SQS long polling/batched receipt deletion, removes the fixed rdkafka
partition key, and restricts rskafka cursor reset to offset-out-of-range errors.
The original findings above describe the audited commit, not the patched state.

Focused regressions cover invalid JetStream limits, notification outages without
job delays, lost wakeups, error observability, task ownership on drop, RabbitMQ
consumer cancellation, and recovery after an earlier shard fails. The shared
broker API now requires owned notification implementations (`'static`); this is
needed because background tasks outlive individual method calls.

Still outside the patch: arbitrary-clock-skew/commit-time lease enforcement for
JetStream; orphan-payload reconciliation; stronger deployment failover guarantees;
and measured attribution of PostgreSQL/JetStream capacity. Those are documented
limitations or investigations, not silently reported as fixed by these changes.
