# JetStream write cost and payload-size attribution

Runtime baseline: `c472a04`. This experiment adds opt-in publish-work counters
and post-workload server-state capture. It changes no batch limit, durability
setting, storage layout or queue transition. It is diagnostic evidence, not a
64 KiB capacity pass or an implemented storage redesign.

## Controlled comparison

Four fresh, colocated, three-node NATS 2.15.0 Docker fixtures ran in small–large–
large–small order. All retained file storage, R3, always-fsync, 16 buckets, a shared
client connection, four workers per tenant, 100 tenants offering ten jobs/second,
32 in-flight offers per tenant, 60 seconds of offers and five seconds of drain.
Payloads were unique per tenant and sequence. Both variants used the same release
binary with diagnostic counters/timings and server-stack sampling enabled.

| Trial | Payload | Accepted / completed / verified | Elapsed | Gate |
|---|---:|---:|---:|---|
| 1 | 1 KiB | 60,000 / 60,000 / 60,000 | 60.071 s | Pass |
| 2 | 64 KiB | 50,950 / 50,950 / 50,950 | 65.001 s | Fail |
| 3 | 64 KiB | 51,132 / 51,132 / 51,132 | 65.004 s | Fail |
| 4 | 1 KiB | 60,000 / 60,000 / 60,000 | 60.058 s | Pass |

Every run recorded zero operation errors. Large-payload failures were overload
rejections. The smaller workload does not satisfy the real 64 KiB target.

## Durable work per job

| Trial | Acknowledged records | Acknowledged publishes/batches | Value bytes | Rejected attempts |
|---|---:|---:|---:|---:|
| 1 | 240,000 | 95,489 | 161,767,536 | 1 |
| 2 | 203,800 | 18,508 | 3,424,249,368 | 0 |
| 3 | 204,528 | 19,153 | 3,436,482,057 | 0 |
| 4 | 240,000 | 95,514 | 161,767,500 | 1 |

Each completed job wrote exactly four records: immutable payload, initial metadata,
claim metadata and completion metadata. Enqueue's two records are one atomic
operation; concurrent operations are further coalesced. All three replicas'
post-verification stream sequences agreed, and the sum over the 16 unique streams
matched the acknowledged record count exactly. Replicas must not be summed as
though they were independent logical records. These fresh fixtures started with
empty streams and performed no purges.

The 1 KiB jobs wrote approximately 2,696 value bytes each; 64 KiB jobs wrote
approximately 67,208. Large runs averaged 0.363–0.375 acknowledged publishes/batches
per completed job, versus 1.591–1.592 for small runs. Larger payloads did not cause
more commits per job or repeated conflicting writes in these observations.

At the target rate, 64 KiB jobs therefore require about 67.2 MB/s of logical value
writes before replication. Three copies represent about 201.6 MB/s of logical
replicated values. **This is not measured physical disk bandwidth**: Raft/WAL,
stream files, headers, retention, network framing and payload reads add work;
server batching can also combine writes. Four records or one acknowledged batch
do not correspond to four or one fsync calls.

## Where time grows

| Mean client-observed interval | 1 KiB trial 1 | 64 KiB trial 2 | 64 KiB trial 3 | 1 KiB trial 4 |
|---|---:|---:|---:|---:|
| Atomic commit | 14.55 ms | 163.20 ms | 153.94 ms | 14.69 ms |
| Enqueue batch queue | 5.32 ms | 2,202.24 ms | 1,642.83 ms | 7.67 ms |
| Metadata batch queue | 10.70 ms | 32.35 ms | 30.71 ms | 10.78 ms |
| Payload read | 2.01 ms | 73.85 ms | 64.76 ms | 1.90 ms |

These inclusive intervals overlap and must not be added. Atomic timing includes
transport and server scheduling as well as persistence. Payload reads overlap
claims. Earlier server execution traces located substantial waiting inside Raft
`storeToWAL` and `os.File.Sync`; those are separate traces, not measurements of
all latency in these four runs.

The controlled variable is payload size, but this also changes automatically
selected batch shapes. The result locates a strong byte-volume sensitivity in the
current adapter/server/storage combination. It does not separately quantify disk,
network, JSON payload reads, stream locking or hardware-independent adapter cost.
It rules out neither server tuning nor an adapter layout improvement. It does
rule out a hidden retry storm or extra logical writes as the explanation for
these particular large-payload misses.

## Engineering decision

Do not replace the portable queue state machine or weaken durability. Stop
adjusting batch sizes in the shared payload/metadata stream as the primary fix.
The next architectural candidate is **separate immutable payload storage from
mutable job metadata**, first using separate NATS buckets on the same deployment.
This is a proposed experiment, not a released feature or a promised capacity fix.

- Keep tenant scoping, scheduling, priority, idempotency, cancellation, history,
  lease fencing and the public queue API intact.
- Acknowledge payload durability before publishing discoverable metadata. An
  uncertain payload write must not publish metadata or be blindly replayed.
- Give bulk payload publishing separate batching from small metadata transitions.
  Both stores retain the same replication and fsync requirements.
- Persist and validate the storage-layout binding. Do not silently open existing
  inline ledgers with a different layout; migration and reopening must be tested.
- Handle dedupe losers and failed/uncertain metadata publication through safe
  orphan reconciliation. Never remove a payload a committed job may still need.
- Test crash boundaries, stale owners, restart/reopen, dedupe, cancellation,
  retention and integrity, then run matched full-target capacity comparisons.

Separate streams isolate logical write queues, not the physical disk. If that
candidate does not improve the target, reject it rather than adding complexity.
Any later external payload-store option must be explicit and provider-neutral;
no application should be forced onto S3 or a second technology stack.

## Measurement scope and validation

`stage_timings.nats_publish_work` counts the batch-writer path, including its
single-record fallback. It does not count unrelated archive/purge operations or
non-batched backends. Value bytes exclude protocol headers and replication.
`errors` counts returned attempt errors; the difference between attempts and
known results may include canceled or still-running attempts. Acknowledgements
are counted only after validation, not on local frame submission or staging.
Server snapshots are taken after verification, outside the timed capacity window;
in other workloads late commits can make their counts differ from the earlier
client snapshot. Snapshot collection errors are saved separately and must not be
interpreted as zero writes. The monitoring endpoint is the documented
[NATS /jsz endpoint](https://docs.nats.io/learn/monitoring/monitoring-endpoints).

All 64 non-live library tests passed with diagnostics enabled. Clippy with
warnings denied, formatting and Python syntax validation passed. The four live
capacity runs exercised actual R3 file storage and payload verification. No paid
resources were created. Raw logs, per-replica snapshots, binary hashes, runner and
machine-readable cross-checks are in the task workspace under `dogrs-write-cost`.
