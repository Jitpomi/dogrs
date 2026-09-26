# Production readiness gates

The hosted acceptance suite is not a production sign-off. The agreed launch target
is 10 jobs/second per tenant, 100 tenants (1,000 jobs/second aggregate), payloads up
to 64 KiB, and recovery after a five-minute outage. These are acceptance targets,
not promises that a free hosted database can provide that capacity.

## Capacity gate

`dog-examples/hosted-system/run_capacity.py postgres` uses a bounded open-loop
arrival schedule. Submissions which exceed the 16-request in-flight limit count
as failures; they are not silently queued in the load generator. Each payload
stage retains previous terminal history. Every accepted ID is checked against
its terminal state and synthetic business effect. The stage deadline allows five
seconds of drain after offered traffic; terminal-observation time includes status
read overhead, so this is a conservative application gate. Business-effect
throughput is reported separately and is not an acknowledgement latency metric. The workload stops at the first
failed stage to avoid exhausting a free service. No test initiates payments.

```sh
cargo build -p hosted-system --locked
export DOGRS_SYSTEM_BINARY="$PWD/target/debug/hosted-system"
export DOGRS_SECRETS_DIR=/absolute/private/directory
export DOGRS_REPORT_DIR=/absolute/results/directory
python3 dog-examples/hosted-system/run_capacity.py postgres
```

The first hosted single-tenant gate failed: of 100 offered jobs over 10 seconds,
32 were accepted and completed, while 68 exceeded the client concurrency bound.
Accepted jobs finished their business effects in 35.794 seconds; enqueue p95 was
18,488.8 ms. No duplicate execution attempts were observed in those 32 jobs.
This is a failed capacity gate, not a successful 10-job/second benchmark. Later
payload stages and the 100-tenant gate were not run in that failing workload.

PostgreSQL's former client mutex and whole-tenant JSON ledger have been replaced
by indexed per-job rows, binary payloads, pooled connections, atomic SQL enqueue
and claim operations, and database-authoritative lease transitions. Status polling
uses the portable `get_snapshot` API to avoid fetching payloads. Migration is
explicit and fences old binaries; see [PostgreSQL upgrade](../dog-queue/POSTGRES.md).
The original failed runs remain historical evidence. Updated benchmarks must
record worker counts, payload sizes and test duration; the 100-tenant aggregate
requirement remains a separate gate.

The optimized v2 application, with two processes and eight workers per process,
accepted and completed all 600 one-KiB jobs offered at 10/second for one minute.
Business effects finished by 60.548 seconds and terminal verification by 61.278
seconds; no duplicate attempts were observed. The earlier single-record polling
benchmark spent additional time making hundreds of verification requests. The
portable bounded `get_snapshots` API now verifies up to 1,000 IDs per request.

The newer two-round-trip fenced PostgreSQL mutations passed the hosted 64 KiB
gate twice: 300/300 jobs over 30 seconds (terminal verification 32.871 seconds,
enqueue p95 756.78 ms), then 600/600 over 60 seconds (terminal verification 61.420
seconds, p95 276.05 ms). Both used two processes with eight workers each and had
zero client overloads or duplicate attempts. This closes the previously failing
single-tenant gate for those observed runs, not every deployment.

The native queue-level harness separately validated 100 tenants at 10 jobs/second
each with 1 KiB payloads: PostgreSQL and Redis each completed 30,000/30,000 over
30 seconds; one-node JetStream completed 10,000/10,000 over 10 seconds. It validates
real persistence APIs, payload integrity, tenant isolation and terminal state,
without HTTP or external payment effects. These local results do not establish
that a free hosted instance can deliver the same throughput. The initial Redis
capacity instance did not establish AOF durability, and one-node JetStream did
not establish failover; controlled durable profiles are separate tests.

The combined 100-tenant, 64 KiB workload is a separate gate. Early local PostgreSQL
runs failed, including one with 9,996 admitted and 9,188 completed by the 15-second
deadline. Do not combine separate small-payload aggregate and large-payload
single-tenant passes into a claim that the combined target passed.

Reproduce disposable durable-profile capacity tests (100 tenants, 10 jobs/second
each, bounded admission, five-second drain):

```sh
cargo build -p hosted-system --release --features redis,nats --locked
export DOGRS_SYSTEM_BINARY="$PWD/target/release/hosted-system"
DOGRS_CAPACITY_WORKERS=1 python3 dog-examples/hosted-system/run_recovery.py postgres \
  --capacity --seconds 30 --bytes 65536 --report-dir /absolute/results
```

### Combined durable-profile results

On the Linux CI runner, Redis AOF/always/noeviction completed and verified all
10,000 full 64 KiB jobs across 100 tenants over ten seconds (including zero
admission drops or duplicate attempts). This is a short controlled load pass,
not a sustained soak or a certification of the hosted Aiven service.

PostgreSQL and replicated JetStream have **not passed** that combined gate.
PostgreSQL experiments with larger shared buffers and uncompressed TOAST storage
did not close the gap. The later capacity fixture records an explicit 1 GiB
shared-buffer / 4 GiB WAL profile, leaving durability enabled. Producer admission is explicitly
tunable instead of hard-coding a pool fraction. On local NATS 2.15.0 with 16 buckets,
three replicas and sync-always, the corrected hint handling eliminated the earlier
lookup errors, but only 7,451 jobs were admitted and 1,915 completed by the deadline.
The sustained throughput target remains a release blocker; failed runs must stay
visible alongside passing unit/contract/recovery tests.

## Controlled recovery tests

`dog-queue/tests/production_faults.rs` is explicitly restricted to loopback
PostgreSQL. The partition test cuts established connections and rejects new
connections through a test proxy. After the specified outage, the same backend
instance must reconnect, preserve the job, reject the expired owner's completion,
reclaim the job and complete the next attempt.

```sh
DOGRS_PARTITION_SECONDS=300 cargo test -p dog-queue --features postgres \
  --test production_faults postgres_recovers_after_partition_and_fences_expired_owner \
  --locked -- --ignored --nocapture
```

Supply `DOGRS_POSTGRES_URL` privately. The snapshot test takes a separate
`DOGRS_RESTORED_POSTGRES_URL` pointing at a `dogrs_restore_*` database restored
from a dump taken while the partition fixture is in flight. It checks recovery
through the public DogRS backend API, rather than checking only that restore
commands returned success. Drop the isolated restore database after testing.
Neither test establishes provider failover behavior or a disaster-recovery RPO.

The new disposable provider-process tests also cover PostgreSQL and Redis
SIGKILL/restart with persisted files, and loss of the actual leader of a
three-replica JetStream stream while the old leader stays down. Each preserved
200 acknowledged 64 KiB payloads, rejected 50 expired owners and recovered 150
unfinished jobs through the same backend instance. The tested Redis configuration
uses AOF/always/noeviction; JetStream uses file storage and sync-always.
See [KV storage](../dog-queue/KV-STORAGE.md) for limits and reproduction.

## Outstanding gates

- Sustained aggregate load, large retained histories, fairness, latency spikes,
  and all payload stages at the agreed traffic rate.
- Clock skew and clock jumps: PostgreSQL now uses database time for ownership;
  Redis uses server TIME; native JetStream still consults the process clock, and authoritative server clock
  jumps remain an operational test requirement. A passing no-skew lease test does not certify distributed clock safety.
- Equivalent controlled recovery/failover evidence for each supported deployment.
- A durable Redis deployment: the tested Aiven Valkey service has AOF disabled.
  Its application recovery result cannot establish lossless server-crash recovery.
- TypeDB application-level job/effect recovery. The existing live transaction
  and atomic-schema tests are narrower and remain useful separate evidence.
- The application-specific Mercury workflow. Mercury documents that webhooks
  are unavailable in sandbox and invoice status has no invoice-specific event;
  a live integration needs an authenticated reachable endpoint and account access.
  See https://docs.mercury.com/reference/webhooks and
  https://docs.mercury.com/docs/invoicing. Do not infer invoice API entitlement
  from the free banking account, and do not upgrade a plan to run these tests.

### Latest correctness checks

A later hosted PostgreSQL 64 KiB run passed 300/300 jobs over 30 seconds, verified
all 300 business effects and completions by 30.744 seconds, and recorded no
overload or duplicate attempts (enqueue p95 447.71 ms). Enqueue remains one atomic statement; optional producer concurrency is
configurable without introducing a second background execution path.

CI exposed a Redis reconnect timeout after restart. The retry cycle is now
bounded to fit below the queue operation deadline; the same-client controlled
restart passed locally after this change. CI recovery remains a required gate,
including the five-minute Redis outage and fresh-container AOF restoration.

The capacity fixture records host/Docker resources and PostgreSQL WAL I/O
statistics. These separate the observed deployment profile from the portable
backend API; they do not turn failing capacity measurements into passes.

### Sustained full-payload result

The 60-second Linux Redis AOF/always/noeviction run offered 60,000 jobs at the
agreed aggregate rate. It admitted 52,712, completed 45,669 before the 65-second
deadline and dropped 7,288 offers at the bounded client admission limit. It
reported no queue errors or duplicate attempts. The server log records repeated
AOF rewrites during the run. The short 10,000-job pass therefore does **not**
close sustained 64 KiB capacity; compaction under load remains a release gate.

The PostgreSQL enqueue-batching experiment preserved transaction correctness but
did not close capacity and was removed rather than expanding the public API.
The ordinary single-statement path and optional producer cap remain. A separate
replicated-NATS 1 KiB run failed in fixture setup due to a repeated host port;
the allocator now ensures uniqueness and tracks containers before starting them.
That setup failure is not a backend capacity measurement.

The final native fixture makes admission concurrency explicit:
`DOGRS_CAPACITY_INFLIGHT` defaults to 32 outstanding requests per tenant (bounded
1–64), versus the earlier 16. At the offered 10 jobs/second per tenant this
provides 3.2 seconds of bounded in-flight capacity; the five-second drain deadline
is unchanged. The selected value is included in every result. Replicated-NATS
capacity runs use two workers per tenant to overlap independent jobs; PostgreSQL
and Redis use one. Offered rate, payload size, expected count and all correctness
checks remain unchanged. Earlier failed profiles remain separate evidence.

### Queue hot-path fixes

Binary PostgreSQL, Redis and JetStream enqueue transitions construct metadata
without copying the submission payload into temporary records. The original
bytes are still persisted and verified by the backend contract tests.

JetStream validates legacy storage on each tenant's first use. Concurrent first
uses share validation; failures remain retryable. Stop all old writers before
migration, as with the Redis backend. Empty JetStream claims can wait up to
50 milliseconds for tenant-specific discovery notifications, registering the
waiter before checking the index. Authoritative reads and revision CAS still
control ownership; notifications never grant a lease. The bounded wait avoids
returning immediately while a remote submission's discovery event is in flight.
