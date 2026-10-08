# JetStream admission attribution after metadata coalescing

These are local diagnostic observations, not a passed production capacity gate.
Runtime baseline: `07fb432` (also the runtime in documentation-only `3d1dbd3`).
Target remains 100 tenants × 10 jobs/second, unique 64 KiB payloads, 60 seconds of
offers and five seconds of drain. Production fixtures retain three replicas,
file storage and `sync_interval: always`. Replicas are colocated; independent
failure domains are not certified.

## Direct admission controls

Fresh Docker fixtures ran sequentially with 16 buckets and shared connections:

| Path | Accepted / payloads verified | Admission elapsed | Operation errors |
|---|---:|---:|---:|
| Native payload create only | 59,579 / 59,579 | 62.344 s | 0 |
| Native payload create then metadata CAS | 57,039 / 57,039 | 62.526 s | 0 |
| DogRS admission, without consumers | 57,797 / 57,797 | 62.215 s | 0 |

All missed admission's target through overload rejections. Native layout uses
individual writes, whereas DogRS uses atomic coalescing; it is a write-shape
control, not an identical protocol implementation. Native and DogRS verification
paths also differ. Sequential runs and changing leader placement permit host
variation. These results do not establish an exact overhead percentage, but they
do show that a target miss also occurs without DogRS queue bookkeeping.

## Split the acknowledgement interval

New opt-in probes observe replies already required by the atomic protocol. They
add no network requests and never treat staging as success. `first_staging_wait`
measures from completion of local frame submission until the first observed empty
staging reply. `after_staging_wait` measures from the last observed staging reply
to a validated final acknowledgement. They include client scheduling and network
handling, are not isolated disk measurements, and may have different sample
counts when no staging reply is observed before completion. Pooled empty replies
carry no batch ID, so these probes are diagnostic observations, not proof of a
particular frame's server processing time.

Two instrumented full-queue runs used four workers per tenant:

| Connections | Accepted / completed / verified | Errors | First staging mean | After staging mean | Atomic commit mean |
|---|---:|---:|---:|---:|---:|
| Shared | 49,942 / 49,942 / 49,942 | 0 | 86.40 ms | 75.28 ms | 166.01 ms |
| Per bucket | 52,560 / 52,560 / 52,560 | 0 | 46.38 ms | 77.68 ms | 136.66 ms |

Neither passed capacity. Splitting connections did not remove the post-staging
wait. Earlier attribution of the entire final-acknowledgement interval to durable
storage was too strong: it also includes transport and server scheduling.

## Server execution trace locates a concrete persistence cost

A further diagnostic run accepted, completed and verified 50,745 jobs with zero
operation errors. Five-second Go execution traces were captured concurrently from
all three NATS 2.15.0 servers during sustained load. `go tool trace -pprof=syscall`
and `go tool pprof` reported:

| Node | Fsync events | Aggregated fsync delay | Mean fsync duration | Share of recorded syscall delay |
|---|---:|---:|---:|---:|
| n0 | 1,722 | 28.355 s | 16.466 ms | 91.44% |
| n1 | 1,727 | 28.931 s | 16.752 ms | 90.12% |
| n2 | 1,728 | 28.880 s | 16.713 ms | 91.75% |

These are concurrent goroutine wait totals, **not CPU percentages, request-latency
percentages, or wall-clock durations**. Approximately 24.7–25.5 seconds of each
node's aggregated syscall delay ran through Raft `storeToWAL`. The traced path is:

`raft.storeToWAL → fileStore.StoreMsg → storeRawMsg/writeMsgRecord → msgBlock.flushPendingMsgsLocked → os.File.Sync → syscall.Fsync`.

This matches the pinned upstream [Raft implementation](https://github.com/nats-io/nats-server/blob/v2.15.0/server/raft.go)
and [file store](https://github.com/nats-io/nats-server/blob/v2.15.0/server/filestore.go).
It identifies a significant server-side persistence bottleneck. It does not prove
that every remaining millisecond is storage, nor that the adapter is optimally
designed for every workload. DogRS still generates the durable writes being timed.

## Controls that did not fix the target

With four workers and unchanged execution limits, four buckets completed and
verified 47,569/47,569 accepted jobs; eight buckets completed and verified
52,279/52,279, both with zero errors. Neither met 60,000. No shard default changed.

A **diagnostic-only**, delayed-fsync (`sync_interval: 2m`) fixture accepted,
completed and verified 57,003 jobs, with zero errors, still missing 2,997 offers.
That weaker setting is not retained or counted as a production pass. Its failure
to reach the target also prevents blaming the entire shortfall on per-commit
fsync alone.

A native macOS three-process NATS 2.15.0 fixture, retaining R3 and always-fsync,
was substantially slower: 14,380 accepted, 13,837 completed in the window, one
uncertain submission deadline and 47 late offers. It did not fix the problem and
cannot substitute for the Docker or Linux results. The first native setup attempt
failed before load because its tenant prefix did not meet the fixture guard;
that setup error was corrected before this recorded run.

## Decision

Retain the validated metadata collection fix and conservative execution defaults.
Do not weaken durability, inflate the capacity deadline, or call higher acceptance
counts a pass. Worker concurrency explains part of the previously unfinished
backlog; the traced Raft persistence path is a concrete remaining cost. We have
not established a further safe DogRS code change that removes this bottleneck or
passes the full target. Faster storage or a redesign that reduces durable writes
would require another controlled test; neither is certified by this report.

Raw reports, verified native binary release metadata, runner scripts and traces
are in the task workspace under `dogrs-admission-attribution-3d1dbd3`,
`dogrs-staging-profile`, `dogrs-staging-per-shard`, `dogrs-topology-after-batching`,
`dogrs-relaxed-fsync-diagnostic`, `dogrs-server-execution-trace`, and
`dogrs-native-nats-validation`. No hosted paid resources were created.
