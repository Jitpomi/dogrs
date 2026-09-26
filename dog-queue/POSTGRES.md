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
Worker wall clocks do not decide lease ownership. Caller-specified absolute
`run_at` and retry timestamps still require the application to supply correct
UTC times. Clock jumps on the database host are an operational concern; this
adapter does not claim to solve arbitrary changes to the authoritative clock.

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
