# JetStream separate payload-storage experiment

Baseline: `41ed477`. Decision: reject this prototype; retain the existing storage
layout. No runtime changes from this experiment are shipped.

## Candidate and controls

The candidate used a second file-backed KV bucket per shard for immutable
payloads. It acknowledged payload persistence before publishing discoverable job
metadata, with expected-revision checks retaining ownership fencing. Metadata
continued through its existing writer. The payload writer used an 8 ms collection
window, a 1 MiB soft batch target, a 2 MiB hard limit and two concurrent batches.
The ordinary layout retained atomic payload-plus-metadata enqueue.

Each comparison used the same release binary, with only the experimental layout
flag changed within a series. Diagnostics were disabled. Fresh colocated
three-node NATS 2.15.0 Docker fixtures retained R3, file storage and always-fsync.
Both layouts used 16 shards and a shared connection; the split layout therefore
had 32 streams versus 16. All streams shared the same physical storage system.
The workload remained 100 tenants offering ten jobs/second each, unique 64 KiB
payloads, 32 in-flight offers per tenant, 60 seconds of offers and five seconds of
drain. Each series ran inline–split–split–inline. Worker counts differed only
between the two series.

This experiment changes both storage separation and payload batching. It does
not isolate either variable independently, and does not measure a deployment
with separate physical disks.

## Results

| Workers/tenant | Trial | Layout | Accepted | Completed by deadline | Verified later | Errors |
|---:|---:|---|---:|---:|---:|---:|
| 4 | 1 | inline | 47,788 | 47,788 | 47,788 | 0 |
| 4 | 2 | split | 57,274 | 52,063 | 52,220 | 0 |
| 4 | 3 | split | 53,691 | 50,285 | 50,293 | 0 |
| 4 | 4 | inline | 49,398 | 49,398 | 49,398 | 0 |
| 8 | 1 | inline | 49,383 | 49,383 | 49,383 | 0 |
| 8 | 2 | split | 48,661 | 48,661 | 48,661 | 272 |
| 8 | 3 | split | 51,208 | 51,208 | 51,208 | 0 |
| 8 | 4 | inline | 49,088 | 49,088 | 49,088 | 0 |

All runs failed the unchanged 60,000-job capacity gate. Verified counts are
post-window observations: late terminal transitions do not count as on-time
completion. Zero operation errors does not imply zero overload rejections.
The eight-worker split run with 272 errors returned enqueue timeouts whose commit
outcomes were unknown. Verifying acknowledged jobs does not resolve those
unacknowledged operations or prove that they had no durable effects.

## Correctness checks and limitations

All 73 library tests passed with live tests included. The prototype-specific test
checked payload integrity, active-key deduplication, completion, absence of
payload records from the metadata bucket, and rejection before metadata became
discoverable when payload storage was unavailable. A separate R3 always-fsync
recovery test killed the active stream leader and passed while that leader stayed
down. These colocated tests do not certify independent provider failure domains.

The prototype did not implement a persisted layout binding, safe migration or
reopening, or orphan reconciliation after uncertain metadata publication. Those
would be prerequisites for a release even if capacity had improved. Payload-first
publication deliberately adds another acknowledgement dependency; separate
logical streams do not remove the shared persistence cost.

## Conclusion

At four workers, separation increased admission but left an unfinished backlog.
Increasing to eight workers did not establish a full-target pass and introduced
uncertain enqueue timeouts in one candidate run. This specific implementation
has no demonstrated reliable capacity benefit that justifies its additional
storage and recovery complexity. Its runtime changes were restored to baseline.

These observations do not prove that all separated-storage designs fail, that
NATS is inherently slow, or that DogRS has no remaining optimization opportunity.
Together with the earlier write-volume and server traces, they show sensitivity
to large durable payloads in this adapter/server/storage combination. They do
not assign the entire remaining delay exclusively to infrastructure or code.

Raw reports, runner scripts, binary hashes and the rejected patch are preserved
in the task workspace under `dogrs-split-prototype` and `dogrs-split-workers8`.
No paid resources were created. Test fixtures were removed after each run.
