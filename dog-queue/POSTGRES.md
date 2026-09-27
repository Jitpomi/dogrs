# PostgreSQL ledger

PostgreSQL is an optional adapter. Enable `dog-queue/postgres`; applications using
Memory, Redis, JetStream, or a custom `QueueBackend` do not need PostgreSQL or bb8.
Brokers accept an `Arc<dyn JobLedger>`; applications can implement that public
contract for another durable database without modifying DogRS.

## Storage and concurrency

The v2 ledger stores one row per `(tenant, job_id)`, with immutable binary payload
bytes separate from mutable JSON execution metadata. `get_snapshot` returns
metadata without downloading the payload. `get_snapshots` supports bounded batches
of up to 1,000 IDs, preserving order and tenant isolation. Active idempotency is enforced
by a partial unique index scoped to tenant, queue, job type and key. Terminal jobs
release the key. Claims lock a single eligible row with `FOR UPDATE SKIP LOCKED`,
so one busy job does not hold up other eligible jobs. Retention and expired-lease
queries use separate indexes; terminal history is not loaded on every transition.

A bounded bb8 pool replaces the process-wide connection mutex. `PostgresOptions`
selects the pool size and operation deadline. Defaults are four connections and
ten seconds per operation. Account for *all* application instances and their
business-database connections when choosing a pool size. Server statements are
also bounded. A timeout can have an unknown commit outcome: use idempotency keys
and reconcile status rather than blindly creating a new logical job.

Lease checks and transitions read PostgreSQL time after acquiring row locks.
Worker wall clocks do not decide lease ownership. `JobMessage::new` uses the
clock-independent `JobMessage::IMMEDIATE` marker (Unix epoch), which PostgreSQL
resolves to database time when inserting the job. An immediate submission cannot
be delayed merely because its producer's clock is ahead. Explicit `with_run_at`
timestamps remain absolute UTC, even if they appear past-dated to a fast producer.
For an immediate retry, pass `Some(JobMessage::IMMEDIATE)`. Other absolute retry
timestamps still require the application to supply correct UTC times.
Clock jumps on the database host are an operational concern; this
adapter does not claim to solve arbitrary changes to the authoritative clock.

## Fixed storage schemas

`PostgresOptions.schema` selects an isolated storage namespace. Startup creates it
transactionally when needed; every connection, including a reconnect, restores
that schema without falling back to `public`. Names are quoted as identifiers,
limited to 63 bytes, and cannot name PostgreSQL system schemas. Creating a new
schema requires database CREATE permission; an existing schema needs the normal
table/function/trigger permissions. `None` preserves the connection's configured
`search_path` and existing storage behavior.

Separate schemas have separate physical job and large-payload tables. The
provider-neutral `ShardedBackend` can route tenants over a fixed list of these
backends. For example, four backends with distinct schema names can share a total
64-connection budget using these options for each:

```rust
# use dog_queue::backend::postgres::PostgresOptions;
let options = PostgresOptions {
    schema: Some("jobs_0".into()), // jobs_1, jobs_2, jobs_3 for the other stores
    max_connections: 16,
    batch_concurrency: Some(1),
    ..Default::default()
};
```

Place their `Arc<PostgresBackend>` values in a fixed order in
`ShardedBackend::new`. This keeps the total producer limit at 48 and executing
claim/completion statements at four per kind across the four stores. Pool budgets
multiply across application instances. This is a supported topology, not a
throughput guarantee; qualify it on the deployment's actual workload.

Schema names, shard count and shard order identify persisted storage. Opening a
different schema does not move existing jobs. Drain or perform a verified offline
tenant migration before changing a live routing topology.

## Offline upgrade from the v1 tenant ledger

1. Back up the database and stop **every old API and worker**. A running old
   handler can still produce business effects even if its later acknowledgement
   is rejected, so this is not a rolling migration.
2. Connect using `PostgresBackend::new_with_tls_options`, passing the same
   certificate-validating TLS connector and `PostgresOptions { migrate_legacy:
   true, operation_timeout: Duration::from_secs(300), ..Default::default() }`.
   Choose a longer migration deadline for a larger database.
3. Verify migrated job status, payloads and lease ownership before starting the
   upgraded applications. Use the normal constructor on subsequent starts.

The ordinary constructor refuses a nonempty v1 ledger until migration is explicitly
requested. Migration is one transaction under the existing schema advisory lock
and an exclusive legacy-table lock. It preserves IDs, tenant membership, history,
results and lease tokens, and marks migration complete exactly once. A trigger
rejects subsequent v1 writes so older binaries cannot silently create a second
ledger. The v1 snapshot is retained, not deleted. Reopening does not re-import it.

Migration requires table/function/trigger creation privileges for the schema owner.
For rollback, stop all upgraded writers and restore a consistent pre-upgrade
backup. Merely removing the v1 fence would discard v2 progress and is unsafe.

## Validation

The backend shares the cross-connection queue contract with Redis and JetStream.
`tests/postgres_rows.rs` additionally checks concurrent claims, locked-job
skipping, explicit migration and old-writer fencing. `tests/production_faults.rs`
checks controlled local connection loss and restoration. Hosted benchmarks are
separate evidence; connection pooling alone does not guarantee a throughput level.

Completion uses one atomic SQL statement. It locks the row before reading database
time, validates status/token/expiry, and commits the result in that statement.
Retry, heartbeat and cancellation use metadata compare-and-swap with a database
clock check after the row lock. Producer admission is bounded before connection
acquisition, so blocked submissions cannot fill the worker connection queue.
`PostgresOptions.enqueue_concurrency = None` selects an automatic limit: reserve
one quarter of the pool, at least one connection when the pool has more than one.
A 64-connection pool therefore admits at most 48 concurrent submissions. Explicit
limits from 1 through `max_connections` override this policy; using the full pool
is appropriate for a producer-only backend. Tune explicit limits against measured
latency. A small cap on a high-latency connection can reduce throughput.


Concurrent completions are coalesced into bounded SQL batches (at most 64 requests,
256 waiting requests, and at most four executing statements per backend). There
is no collection timer on a quiet queue. Each request independently checks its
tenant, token, status and lease after row locking; responses are sent only after
the statement commits. Duplicate requests for the same tenant/job are placed in
separate statements so they cannot both succeed from one pre-update snapshot.
Invalid leases do not roll back valid requests in the same batch. Only explicitly
aborted deadlock/serialization failures may be retried; network failures retain
unknown-commit semantics.

`PostgresOptions.completion_batch_size` accepts 1 through 64 and defaults to 64.
Set it to 1 to use independent commits, avoiding cross-job row-lock waiting within
a batch. Batching is a throughput/latency choice, not a change to durability.

Concurrent claims from distinct tenants use a separate bounded dispatcher with
256 waiting requests and at most four executing statements per backend.
`PostgresOptions.claim_batch_size` defaults to 16 and accepts 1 through 64; set it
to 1 for independent claims. Each SQL batch includes at most one request per
tenant, including when callers request overlapping queues. This prevents two
requests in the same statement from leasing the same row. Row locking uses
`SKIP LOCKED`, and ownership is returned only after the statement commits.

`PostgresOptions.batch_concurrency` optionally limits executing statements per
dispatcher (claim and completion separately), from one through the pool size.
The default remains one quarter of the pool, capped at four. Use explicit budgets
when composing multiple backends so adding stores does not silently multiply
worker statement concurrency.

PostgreSQL cannot store NUL characters in text or JSONB strings. Tenant, queue,
lease, result and metadata inputs are checked before submission so an invalid
request cannot abort valid peers in a batch. Binary payloads may contain NUL bytes.
