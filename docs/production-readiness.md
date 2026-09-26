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

The 64-KiB gate remains unresolved in this hosted environment. In the optimized
three-size run, 79/100 offers were admitted at 64 KiB; 21 exceeded the client
concurrency bound. All admitted jobs completed. Earlier intermediate runs admitted
all 100 but exceeded the drain deadline. Do not treat a smaller-payload pass as
certification of the full target or attribute every failure to provider capacity.

A separate no-queue transport probe sends binary parameters to PostgreSQL without
writing data. It measured p95 latencies of 108 ms, 2,437 ms and 700 ms for 1/16/64
KiB in one run. This establishes variability on the test connection, not its cause.
The 100-tenant aggregate workload has not been established.

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

## Outstanding gates

- Sustained aggregate load, large retained histories, fairness, latency spikes,
  and all payload stages at the agreed traffic rate.
- Clock skew and clock jumps: PostgreSQL now uses database time for ownership;
  other ledgers still consult the process clock, and authoritative server clock
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
