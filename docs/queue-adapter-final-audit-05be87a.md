# Final adapter correctness review

Reviewed commit: `05be87abf8f95c6df83896fa1edeb6670b998c08`, PR #24.
Date: October 7, 2026.

Verdict: **changes requested for notification shutdown correctness**. This is a
source/control-flow audit against platform documentation and the installed SDK
source. The findings below were not reproduced in new live fault-injection runs.
They concern adapter lifecycle, not evidence of lost durable jobs or an explanation
of the direct-ledger capacity failures. No implementation changes were made during
this final review.

## Findings

### P2: RabbitMQ cancellation does not settle prefetched deliveries

Location: `dog-queue/src/backend/rabbitmq.rs:81`.

The adapter uses manual acknowledgements and prefetch 32. Shutdown drops the
consumer while retaining the supplied channel. When shutdown interrupts receive,
or the consumer has buffered deliveries, cancelling the subscription does not
requeue those unacknowledged deliveries. They can remain held on the open channel.
Lapin's consumer drop schedules cancellation; this method also does not await a
protocol-level cancellation acknowledgement.

[RabbitMQ explicitly documents](https://www.rabbitmq.com/docs/consumers#cancelling-a-consumer)
that cancellation leaves in-flight deliveries unaffected and that closing the
channel requeues them. Merely observing consumer count zero is therefore an
insufficient shutdown test.

Fix: give the receive side an explicitly owned channel that can be closed safely,
or explicitly settle every outstanding delivery and await cancellation. Do not
close a caller-shared channel without changing the ownership contract. Test a
prefetch backlog and cancellation between delivery and acknowledgement; verify
unacknowledged counts and subsequent consumption, not only consumer count.

### P2: Pub/Sub and rdkafka omit subscription shutdown

Locations: `dog-queue/src/backend/gcp_pubsub.rs:55` and
`dog-queue/src/backend/kafka.rs:48`.

Both implementations inherit the no-op `Notifications::shutdown`. Stopping the
wrapper tasks retains the Pub/Sub MessageStream and the subscribed StreamConsumer
inside the backend. Thus the public promise to stop notification I/O is not met
while the backend remains alive. Pub/Sub lease management can remain active;
Kafka does not explicitly relinquish its subscription and may retain assignments
until its own timeout/rebalance behavior intervenes. Ledger fallback remains
available, but this is not complete notification shutdown.

The installed `google-cloud-pubsub` 1.5.0 source, in
`src/subscriber/message_stream.rs:265`, provides `shutdown_token()` and explains
that awaiting shutdown flushes pending acknowledgements and arranges redelivery.
The stream's drop guard only runs when the stream itself is dropped. See also
[Google's SDK source](https://github.com/googleapis/google-cloud-rust/blob/main/src/pubsub/src/subscriber/message_stream.rs).
The Kafka client exposes
[Consumer::unsubscribe](https://docs.rs/rdkafka/latest/rdkafka/consumer/trait.Consumer.html#tymethod.unsubscribe);
cancelling a `recv()` future does not call it.

Fix: implement adapter-specific shutdown, with fresh stream creation or
resubscription on restart. Bound shutdown appropriately, and test buffered-message
shutdown, repeated shutdown, and restarting the same backend. Kafka producer
operations already queued inside the client also need an explicit documented
drain/cancellation boundary; aborting a wrapper future alone does not establish it.

### P2: cleanup is skipped when wrapper tasks never started

Location: `dog-queue/src/backend/broker.rs:135`.

The adapter shutdown hook is inside `if let Some(tasks) = self.tasks.take()`.
However, `notifications()` is public and can initialize a RabbitMQ consumer
through direct receive before any enqueue/dequeue starts the wrapper tasks.
Pub/Sub constructs its SDK stream during backend creation, and Kafka subscribes
in its constructor. Calling `shutdown_notifications()` in these states skips
adapter cleanup altogether. This remains a bug even after implementing the missing
hooks above.

Fix: abort and join any wrapper tasks, then always invoke an idempotent adapter
shutdown hook. Add a mock-hook regression for shutdown before task startup and
integration coverage for a directly initialized consumer.

## Checks that remain sound in the patch

- SQS receives at most ten messages and checks per-entry batch deletion failures.
  This matches [AWS's batch API contract](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/APIReference/API_DeleteMessageBatch.html),
  including partial failure on HTTP 200. Lost/deleted wakeups do not remove jobs.
- The broker checks the authoritative ledger before waiting for a wakeup and
  retains periodic polling. Publication failures cannot undo a committed job.
- NATS rejects zero, oversized and malformed text execution limits before spawning
  dispatcher lanes. Defaults remain two enqueue batches and one update batch.
- rskafka only resets its cursor for offset-out-of-range errors. Removing the
  rdkafka fixed key removes forced key-based concentration; it does not guarantee
  even distribution for every caller's partitioner configuration.
- Sharded recovery attempts healthy shards despite failures elsewhere, bounds
  concurrency, and exposes mixed outcomes through the report API. A timeout may
  still have an unknown remote outcome, as the error states.
- PostgreSQL and Redis ledger implementation files were not changed by this patch.
  The preceding audit's lease, transaction and durability boundaries still apply.
  [PostgreSQL queue locking](https://www.postgresql.org/docs/current/sql-select.html)
  and [Redis script atomicity](https://redis.io/docs/latest/develop/programmability/eval-intro/)
  support those mechanisms, not a universal deployment guarantee.

## Evidence and remaining boundaries

The existing [correctness/recovery CI](https://github.com/Jitpomi/dogrs/actions/runs/37709318448)
passed all 11 jobs at this commit. Its shutdown coverage does not establish the
backlogged/prefetched lifecycle cases above. No new full CI or capacity run was
performed for this source audit.

The separate [capacity run](https://github.com/Jitpomi/dogrs/actions/runs/37709318489)
failed all three gates. This review does not reclassify those results as passes or
attribute them exclusively to infrastructure.

JetStream clock-skew/commit-time lease enforcement and orphan-payload
reconciliation remain the explicitly documented limitations from the preceding
audit. Revision CAS protects against a replaced owner, but does not itself add a
server-time expiry predicate. [NATS KV documentation](https://docs.nats.io/learn/key-value)
describes the underlying revision-based storage model. These limitations were not
fixed by the execution-limit change.

## Follow-up edit and verification

The working-tree follow-up addresses all three findings above. Their descriptions
remain an audit of `05be87a`, not of this subsequent edit.

- Shared shutdown always invokes adapter cleanup, even before task startup.
- RabbitMQ awaits cancellation, settles the prefetched hints, and uses a channel
  round trip to order acknowledgements without closing the caller's channel.
- Pub/Sub streams are lazy, use immediate nack on shutdown, and are explicitly
  shut down and recreated on restart.
- rdkafka unsubscribes during shutdown and resubscribes on the next receive.
- Documentation now distinguishes wrapper/subscription shutdown from SDK connection
  shutdown and producer flushing. Protocol cleanup has a five-second bound and
  logs incomplete cleanup; RabbitMQ requires caller channel closure after failure.

Validation performed locally for this edit:

- All-feature library tests: 62 passed, 7 live tests ignored by that command.
- RabbitMQ 4.1 with PostgreSQL 17: both integration tests passed, including a
  32-message prefetch backlog, shutdown before wrapper startup, repeated shutdown,
  restart, and channel closure checking that no stranded messages reappear.
- Kafka-compatible Redpanda v25.1.9 with PostgreSQL: notification shutdown/restart
  test passed.
- Google Pub/Sub emulator with PostgreSQL: notification shutdown/restart test passed.
- All-feature/all-target Clippy with warnings denied: passed.

These tests do not certify managed-provider outages, IAM or regional failover.
The capacity gate was not rerun. No new capacity or JetStream lease guarantee is
claimed. The edit is local and has not been published to crates.io.
