# Separating framework and provider capacity

`compare_capacity.py` compares four paths sequentially on fresh disposable
PostgreSQL or three-replica JetStream servers. It keeps fsync, replication,
payloads, offered rate, tenant placement and admission bounds fixed. No hosted
resources are created. Run it on an otherwise idle machine with enough free disk
for the selected workload; do not build or run other performance tests alongside it.

```sh
cargo build -p hosted-system --release --features redis,nats --locked
DOGRS_SYSTEM_BINARY=target/release/hosted-system \
  python3 dog-examples/hosted-system/compare_capacity.py \
  --report-dir /absolute/new-report-directory --seconds 60 --repeats 2
```

The existing **Provider capacity** GitHub workflow also supports `mode=attribution`
with a PostgreSQL or NATS backend. Its ordinary pull-request job is unchanged:
60 seconds of full queue acceptance, without diagnostic substitutions.

| Profile | What is measured | What it cannot establish |
|---|---|---|
| `native-payload` | Direct SDK payload persistence and readback | Queue lifecycle capacity or framework correctness |
| `native-layout` | Direct SDK admission with equivalent data layout/writes | Claims, completion, discovery, deduplication races or recovery |
| `dogrs-admission` | Actual DogRS enqueue and record readback, without consumers | Full queue capacity |
| `queue` | Actual enqueue, claim, payload verification, completion and terminal checks | Failure modes outside this workload |

PostgreSQL native-layout deliberately reuses the exact schema and INSERT SQL.
A small difference from DogRS can isolate the Rust admission path, but does **not**
exonerate the shared SQL/schema. The native pool is preconnected; DogRS pool reads
are warmed before admission timing. Both use 64 connections with at most 48
concurrent admissions. JetStream uses 16 buckets, FNV tenant routing, and 16
concurrent admissions per bucket. Native-layout uses one immutable payload write
followed by a metadata CAS. It omits DogRS discovery and duplicate-submission
handling. These are diagnostic baselines, not replacement queue implementations.

All profiles offer 10 jobs/sec for each of 100 tenants, permit 32 in-flight
submissions per tenant, check acknowledged payloads, and retain the five-second
admission/drain limit. The full queue profile uses one PostgreSQL worker or two
JetStream workers per tenant. Odd repetitions reverse profile order. The report
records the binary hash, source revision/dirty state, raw results and provider
resource/query profiles. A missing measurement is a setup/harness failure, never
a capacity pass. An unsuccessful stage makes the comparison process fail.

Interpret the results in layers:

- If native payload persistence cannot sustain the offered rate, the provider
  profile has a limit even without the framework. This does not prove the
  hardware's absolute maximum or rule out better configuration.
- If native-layout passes but DogRS admission repeatedly fails under matched
  conditions, investigate framework admission overhead before blaming hardware.
- If admission passes but full processing fails, investigate the additional
  claim/completion/discovery work and provider resource use. Admission alone
  cannot certify the complete lifecycle.
- Low client CPU does not rule out application serialization or waiting. Check
  repeated latency results and server profiles, rather than selecting a favorable run.

## Preservation under overload

```sh
DOGRS_SYSTEM_BINARY=target/release/hosted-system \
DOGRS_NATS_IMAGE=nats:2.15.0-alpine \
DOGRS_CAPACITY_SHARDS=16 DOGRS_CAPACITY_WORKERS=2 \
  python3 dog-examples/hosted-system/run_recovery.py nats --capacity \
  --seconds 10 --bytes 65536 --overload-drain-seconds 60 \
  --report-dir /absolute/overload-report
```

This freezes the capacity result, then allows a separate bounded backlog drain.
`overload_recovery` reports whether every acknowledged job finished with verified
payloads, one attempt and no operation errors. The original `passed` result and
process exit status remain failed if the throughput gate was missed. This cannot
turn slow processing into a production-capacity pass. Latency summaries explicitly
state when they include recovery. Offers skipped by the load generator's in-flight
bound are reported separately from acknowledged jobs; they were never submitted
to DogRS. A job still queued at the deadline is not evidence of job loss.

Neither these comparisons nor a green functional CI run prove universal absence
of defects. Keep unexplained gaps and untested failure modes open, and retain the
separate sustained-capacity and recovery requirements.
