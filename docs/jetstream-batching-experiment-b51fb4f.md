# JetStream batching and shard experiment

Base: `b51fb4ff54153056c3954f227ff557c08edaa07c`.
Local Mac/Docker experiment, October 7–8, 2026. No CPU/queue profiling.

Hypothesis: raising the soft enqueue coalescing target from 256 KiB to 1 MiB
would amortize final atomic commit acknowledgement costs. The 2 MiB hard bound,
128-message bound, revision fencing, and durability settings were unchanged.

Each trial used fresh three-node JetStream fixtures with three replicas and
`sync_interval: always`, 100 tenants, 10 offers/second/tenant, unique 64 KiB
payloads, 60 seconds of offers, five seconds of drain, two workers/tenant, shared
connections, and unchanged admission/execution limits. Binary hashes and complete
results were retained in the local experiment artifacts. Compilation was finished
before measurements began. These are colocated Docker replicas, not independent
failure domains or a hardware-controlled storage benchmark.

| Order | Buckets | Soft batch target | Accepted | Completed | Recorded errors |
|---|---:|---:|---:|---:|---:|
| 1 | 16 | 256 KiB | 49,054 | 19,264 | 2 |
| 2 | 16 | 1 MiB | 55,547 | 14,898 | 0 |
| 3 | 16 | 1 MiB | 57,223 | 13,291 | 1 |
| 4 | 16 | 256 KiB | 53,551 | 24,652 | 1 |
| 5 | 4 | 256 KiB | 45,586 | 45,586 | 0 |
| 6 | 4 | 256 KiB | 48,413 | 48,413 | 0 |
| 7 | 4 | 1 MiB | 57,854 | 27,456 | 1 |
| 8 | 4 | 1 MiB | 59,177 | 27,094 | 1 |

All trials missed the 60,000-job gate. Recorded errors were verification read
timeouts and, in trial 1, a submission deadline with uncertain outstanding
commits. Larger acceptance counts are not evidence of better end-to-end capacity.

Decision: reject and revert the 1 MiB candidate. It reduced completion throughput
in both tested topologies. There is no retained runtime change from the larger-enqueue-batch experiment.

Four buckets with the existing implementation completed roughly twice as much
work as sixteen on this machine, with all accepted jobs completed and verified.
That is evidence for sizing topology to available storage resources, not proof
that four is universally optimal. The four-bucket trials followed the sixteen-
bucket trials rather than alternating, so time-dependent host effects remain a
confounder. None of these numbers should be substituted for GitHub runner results.

Existing diagnostics locate the large waits in final durable acknowledgement and
adapter queueing. These trials show that simply enlarging enqueue batches worsens
completion throughput; they do not isolate a particular server lock or disk cost.
Do not weaken fsync or replication to relabel a miss as a pass. Further work needs
controlled measurements of fewer state transitions or a native-consumer prototype
that preserves scheduling, priority, cancellation and revision-fenced ownership.

Changing a persistent shard count/order changes tenant placement. Any existing
ledger requires an explicit migration; never change the count in place merely
because a fresh-fixture benchmark is faster.

## Bounded metadata collection window

A follow-up changed only the metadata lane: collect distinct-key transitions
for up to eight milliseconds, rather than stop once two updates arrive in a
one-millisecond window. Existing queued updates still drain immediately up to the
same message/byte bounds. The enqueue target remains 256 KiB. Claims, completions,
lease checks, expected revisions and final acknowledgement requirements are unchanged.

Four fresh 16-bucket trials ran in baseline–candidate–candidate–baseline order.
The release binaries were built before measurement, with diagnostics disabled;
the workload and R3/always-fsync settings above were unchanged.

| Order | Metadata collection | Accepted | Completed | Recorded errors |
|---|---|---:|---:|---:|
| 1 | Baseline | 50,043 | 27,641 | 1 |
| 2 | Up to 8 ms | 51,659 | 44,896 | 1 |
| 3 | Up to 8 ms | 49,812 | 34,728 | 2 |
| 4 | Baseline | 51,178 | 22,359 | 1 |

All four missed the full capacity gate. Errors were snapshot-read or overall
snapshot-verification timeouts; terminal verification was incomplete. Do not
interpret these counts as a successful correctness or recovery run. Both candidate
completion counts exceeded both baselines, supporting retention for separate
correctness validation, but the sample is small and host/storage variation remains.
The first pair's completion p95 fell from 465 ms to 272 ms. This does not identify
a server-internal bottleneck or establish that the remaining gap is infrastructure-only.

This trades up to eight milliseconds of metadata collection latency for greater
coalescing opportunity. It does not remove a job's durable state transitions and
is not a general capacity guarantee. The complete local reports, binary hashes,
and runner are retained under `dogrs-metadata-window-comparison` in the task workspace.

Validation of the retained metadata-window change: 63 library unit tests, seven
live JetStream library tests, and three live integration tests passed. Formatting
and Clippy with warnings denied passed. A separate three-replica, always-fsync
64 KiB recovery fixture passed after killing the active stream leader and keeping
it down (three-second configured outage). This is not certification of a long
partition, independent provider failure domains, or the failed capacity target.

## Follow-up on `07fb432`: CI and concurrency attribution

[Full correctness/recovery CI](https://github.com/Jitpomi/dogrs/actions/runs/37716665765)
passed all 11 jobs. The separate
[ordinary capacity run](https://github.com/Jitpomi/dogrs/actions/runs/37716665742)
reported:

| Backend | Accepted | Completed in window | Seconds | Recorded errors | Gate |
|---|---:|---:|---:|---:|---|
| PostgreSQL | 60,000 | 60,000 | 63.072 | 0 | Pass |
| Redis | 55,784 | 55,784 | 65.005 | 0 | Fail |
| JetStream | 57,274 | 41,208 | 65.008 | 0 | Fail |

Redis rejected 4,216 offers under load; it verified every accepted job. JetStream
rejected 2,726 offers and had unfinished work at the capacity deadline. Neither
failure should be described as an operation-error or data-loss result. The
JetStream-only source change does not establish the cause of Redis's variation.

A [separate instrumented run](https://github.com/Jitpomi/dogrs/actions/runs/37716870315)
accepted 56,938 and completed 48,761, with zero errors, still failing the target.
Mean metadata queue wait was 10.50 ms and execution was 112.18 ms. Across atomic
commits, mean final acknowledgement wait was 111.51 ms of 115.09 ms total. Candidate
selection averaged 0.096 ms; payload reads averaged 24.09 ms and overlap claims.
These are inclusive timings, not additive CPU costs, and this separate runner is
not a controlled before/after comparison. The dominant measured wait remains
final acknowledgement; it does not identify an exclusively infrastructure cause.

Further local trials used the same `07fb432` binary, 16 fresh buckets, shared
connections, unique 64 KiB payloads, 100 tenants at 10 offers/second, 60 seconds
plus five-second drain, R3 and always-fsync. No code/default changes were made.

| Order | Workers/tenant | Enqueue batches/store | Metadata batches/store | Accepted | Completed | Errors |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 2 | 2 | 1 | 47,180 | 32,383 | 0 |
| 2 | 2 | 2 | 2 | 50,921 | 39,534 | 0 |
| 3 | 2 | 2 | 2 | 49,131 | 34,004 | 4 |
| 4 | 2 | 2 | 1 | 47,685 | 32,833 | 0 |
| 5 | 4 | 2 | 1 | 50,186 | 50,186 | 0 |
| 6 | 4 | 2 | 1 | 50,970 | 50,970 | 0 |
| 7 | 4 | 4 | 1 | 54,762 | 53,510 | 1 |
| 8 | 4 | 4 | 1 | 49,810 | 45,220 | 0 |

All eight failed the 60,000-job target. Trial 3 reported an uncertain submission
deadline, two snapshot read timeouts and overall verification timeout. Trial 7
reported an overall verification timeout. Trials 5 and 6 verified every accepted
job, but rejected 9,814 and 9,030 offers respectively.

Trials 1–4 used ABBA ordering. Trials 5–8 were sequential configuration probes,
not randomized controlled measurements. They support worker concurrency as a
contributor to the unfinished backlog on this host, without proving causality
for every timing difference. Increasing execution concurrency did not produce a
reliable full-target pass, so defaults remain two enqueue batches and one metadata
batch per store. Snapshot timeout limits and the capacity gate were not relaxed.

A worker waits for both durable claim and completion. At ten jobs/second per
tenant, two workers allow roughly 200 ms of total mean cycle time before backlog
grows, even before handler time. Size application worker count to measured cycle
latency with headroom. This is separate from the adapter's batch execution limits.
More workers can expose, rather than eliminate, the enqueue/storage bottleneck.

The complete reports and binary hashes are retained in the task workspace under
`dogrs-metadata-concurrency-comparison`, `dogrs-worker-concurrency-comparison`,
and `dogrs-admission-concurrency-comparison`. The 1,000 jobs/second target remains
unproven for JetStream; no native-consumer redesign or weaker durability is implied.
