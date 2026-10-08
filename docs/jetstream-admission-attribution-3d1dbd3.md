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

## Full-queue individual-write control

A further ABBA comparison used the same runtime binary, 16 fresh buckets, four
workers per tenant, shared connections, R3 and always-fsync. It changed only the
fixture's atomic-publishing switch; the adapter consequently used its existing
individual-write or atomic-batching path and their respective default admission
limits (16 versus 128). This compares the supported paths, not an isolated
measurement of protocol overhead with identical concurrency.

| Order | Path | Accepted / completed / verified | Operation errors |
|---|---|---:|---:|
| 1 | Individual writes | 43,526 / 43,526 / 43,526 | 0 |
| 2 | Atomic batching | 50,413 / 50,413 / 50,413 | 0 |
| 3 | Atomic batching | 49,762 / 49,762 / 49,762 | 0 |
| 4 | Individual writes | 44,028 / 44,028 / 44,028 | 0 |

All missed the 60,000-job target. Both atomic trials completed more jobs than
either individual-write trial. Removing batching is not supported as a fix by
this control. Complete reports and binary hashes are in `dogrs-atomic-control`
in the task workspace.

On `9146608`, [correctness and recovery CI](https://github.com/Jitpomi/dogrs/actions/runs/37720462574)
passed all 11 jobs. The separate [capacity run](https://github.com/Jitpomi/dogrs/actions/runs/37720462544)
passed Redis (60,000 accepted and completed), but failed PostgreSQL (53,111
accepted and completed, zero operation errors) and JetStream (31,255 accepted,
21,304 completed, one uncertain submission deadline). These runner results are
separate from the local controlled comparison and do not establish a source
regression from the opt-in diagnostic probes.

## Larger enqueue batches after the metadata fix

The earlier 1 MiB experiment predated the metadata collection fix and used two
workers. To check whether that interaction explained its regression, a new ABBA
comparison changed only the soft batch target from 256 KiB to 512 KiB in the
current implementation, with four workers. The hard 2 MiB and 128-message bounds,
revision fencing, final-acknowledgement requirement and R3/always-fsync fixture
were unchanged. Release binaries had diagnostics disabled and were built before
measurement. Each trial used fresh storage; no other capacity test ran locally
at the same time.

| Order | Soft target | Accepted | Completed within window | Verified terminal afterward | Operation errors |
|---|---:|---:|---:|---:|---:|
| 1 | 256 KiB | 49,456 | 49,456 | 49,456 | 0 |
| 2 | 512 KiB | 50,362 | 37,614 | 37,441 | 2 |
| 3 | 512 KiB | 51,683 | 50,807 | 50,868 | 0 |
| 4 | 256 KiB | 48,084 | 48,084 | 48,084 | 0 |

All four missed capacity. Trial 2 reported an uncertain submission deadline and
a snapshot-read timeout, so verification was incomplete. Trial 3's later terminal
count includes commits observed after the timed workload ended; those are not
on-time completions. Neither candidate completed every accepted job in the window.
The two baselines did, with zero operation errors. The candidate is **rejected and
reverted**: larger admission counts do not establish reliable completion capacity.
Reports, runner and binary hashes are retained under `dogrs-half-mib-control`.

These controls narrow the decision without proving an exclusively infrastructure
cause. Removing batching was slower, while increasing its byte target did not
reliably improve completion. The traced server persistence cost remains concrete;
these experiments establish no further safe runtime fix for the full target.

## Reply isolation and optional staging acknowledgements

A subsequent code review found that pooled reply subjects were reused across
batches. Successful acknowledgements have a batch ID, but errors and empty
staging replies do not. A deterministic live-server protocol test delivers an
old conflict reply after the next batch starts: the previous implementation
returns a conflict for the new batch despite its successful acknowledgement.
Using a unique per-batch reply subject beneath the pooled wildcard subscription
fixes the regression while preserving subscription reuse. This fault-injection
test demonstrates reply isolation; it does not establish that delayed replies
caused any of the capacity misses above.

The same candidate omits optional intermediate staging reply requests on the
standard publishing path. First-frame and final-commit replies remain required;
custom API-prefix routing keeps its existing request path. Expected revisions,
batch bounds, conflict splitting and final durable acknowledgement validation
are unchanged. This reduces protocol reply traffic, not durable writes.

ABBA local trials retained 16 buckets, four workers per tenant, R3, always-fsync,
100 tenants at 10 jobs/second, unique 64 KiB payloads and the 65-second window:

| Order | Path | Accepted / completed / verified | Operation errors |
|---|---|---:|---:|
| 1 | Baseline | 50,200 / 50,200 / 50,200 | 0 |
| 2 | Candidate | 49,562 / 49,562 / 49,562 | 0 |
| 3 | Candidate | 50,275 / 50,275 / 50,275 | 0 |
| 4 | Baseline | 47,606 / 47,606 / 47,606 | 0 |

All four failed capacity through overload rejection. The candidate is retained
for reply isolation and reduced acknowledgement traffic, **not as a demonstrated
throughput improvement**. All 71 library tests, including live JetStream tests,
passed; the new delayed-reply test was also confirmed to fail with the old
reply-subject reuse restored. Clippy with warnings denied and formatting passed.
A separate 64 KiB, R3, always-fsync recovery fixture passed after killing the
active stream leader and keeping it down (three-second configured outage).
This is a colocated local failure test, not provider failure-domain certification.
Runner scripts, logs and binary hashes are under `dogrs-reply-control` in the
task workspace. Opt-in diagnostics were disabled for both release binaries.

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
