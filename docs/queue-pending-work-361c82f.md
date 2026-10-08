# Outstanding-work attribution at 900 jobs/second

Commit `361c82f` adds opt-in client concurrency measurements and Redis stage
measurements. It changes no adapter admission, batching, persistence, worker or
queue semantics. It is instrumentation, not a performance fix.

## GitHub results

All runs offered 54,000 unique 64 KiB jobs across 100 tenants over 60 seconds,
allowing five seconds to drain. Redis used one worker per tenant and AOF
always-fsync. NATS used two workers per tenant, 16 shards, R3 file storage and
always-fsync. Automatic persistence activity remained enabled.

| Run | Backend | Diagnostics | Accepted | Completed by deadline | Verdict |
|---|---|---|---:|---:|---|
| [37757656461](https://github.com/Jitpomi/dogrs/actions/runs/37757656461) | Redis | Off | 54,000 | 54,000 | Pass |
| [37757656461](https://github.com/Jitpomi/dogrs/actions/runs/37757656461) | NATS | Off | 30,339 | 20,466 | Fail |
| [37757706298](https://github.com/Jitpomi/dogrs/actions/runs/37757706298) | Redis | On | 54,000 | 54,000 | Pass |
| [37757710120](https://github.com/Jitpomi/dogrs/actions/runs/37757710120) | NATS | On | 54,000 | 54,000 | Pass |
| [37758285132](https://github.com/Jitpomi/dogrs/actions/runs/37758285132) | Redis | Off | 54,000 | 54,000 | Pass |
| [37758288969](https://github.com/Jitpomi/dogrs/actions/runs/37758288969) | NATS | Off | 54,000 | 54,000 | Pass |

Every passing run verified all terminal jobs and payload integrity, with zero
operation errors, overload rejections or late offers. The final ordinary Redis
and NATS runs finished in 60.059 and 62.014 seconds respectively. The failed NATS
run rejected 22,795 offers and reported a submission deadline with unknown commit
outcomes. Late verification observed 20,571 terminal jobs; this is not an on-time
completion count.

## What the added measurements show

During the Redis diagnostic pass, peak outstanding enqueue operations were 53,
claims 100 and completions 85. Enqueue writes averaged 7.20 ms and metadata writes
8.85 ms. This passing workload did not build a thousands-deep producer backlog.

During the NATS diagnostic pass, peak pending enqueue requests were 607 and
metadata requests 200. Enqueue batch queue wait averaged 123.38 ms; enqueue batch
execution averaged 91.70 ms. Metadata batch queue wait averaged 11.74 ms and
execution 89.99 ms. There were up to 32 executing enqueue batches and 16 metadata
batches. These process-wide counts span all shards, not one bucket.

`active` and `peak_active` count polled client scopes/futures. They do not count
commands confirmed to be in the server, bytes, or durable commits. Pending NATS
requests are groups submitted by callers, not necessarily distinct executed
batches: batching coalesces requests. Cancellation releases the client count
without resolving an uncertain remote outcome. Scope timings include normal and
canceled scope lifetimes; cancellation counts are specifically available for
`measure` futures. Historical `elapsed`-only metrics do not track concurrency;
their zero active/peak values must not be interpreted as an empty queue.
Inclusive concurrent timings overlap and must not be summed as sequential time.

## Decision

Retain diagnostics, not the previously rejected hard-coded producer limit.
Both backends have new ordinary 900/s passes, but there is no demonstrated code
fix for inconsistent capacity. NATS's pass and failure on the same revision are
particularly strong reasons not to claim the latest pass resolves the problem.
These separate GitHub jobs do not isolate hardware, disk contention or scheduler
variation, and do not prove an infrastructure-only cause.

A controlled comparison on the same runner with repeated fresh fixtures is needed
to distinguish instrumentation effects from runner variation. Preserve failures
and the exact workload settings; do not weaken durability or retry until a single
green run appears. Raw artifacts are preserved in the task's temporary artifact
folders, and linked GitHub runs are the source records.

Validation: 64 library tests passed before adding the additional guard regression;
both diagnostic guard tests then passed, including release on early exit/unwind.
The live diagnostic and ordinary runs exercised real Redis/NATS persistence.
