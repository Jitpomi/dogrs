# Capacity runner variance follow-up

This investigation changes the test harness, not queue adapter semantics. The
900 jobs/second target remains 100 tenants × 9 jobs/s, unique 64 KiB payloads,
60 seconds of arrivals and a five-second drain. Redis retains AOF always-fsync;
NATS retains 16 shards, three file-backed replicas and always-fsync; PostgreSQL
retains synchronous commits, fsync and full-page writes.

## Common-runner result

[All six ordinary cases passed on `92ccc5a`](https://github.com/Jitpomi/dogrs/actions/runs/37775708120),
using one AMD EPYC 7763 runner and one release binary without queue diagnostics.
Each case accepted, completed and verified 54,000 jobs, with zero errors,
overload or late offers. Cases ran in the order shown:

| Order | Backend | Elapsed | Gate |
|---|---|---:|---|
| 1 | PostgreSQL | 60.869 s | Pass |
| 2 | Redis | 60.057 s | Pass |
| 3 | NATS | 60.662 s | Pass |
| 4 | NATS | 60.588 s | Pass |
| 5 | Redis | 60.060 s | Pass |
| 6 | PostgreSQL | 60.085 s | Pass |

This establishes all three meeting the target on a common tested environment.
It is not an adapter performance fix or proof of consistent capacity on every
standard hosted runner. Separate matrix failures below remain failures.

## Observed failure mechanism

The earlier [NATS failure](https://github.com/Jitpomi/dogrs/actions/runs/37769311744)
accepted 30,653 and completed 20,513 jobs by deadline. Enqueue p95 was 7.875 s;
claim/completion p95 were about 677/679 ms. Aggregate claim and completion waits
occupied approximately 12,904 worker-seconds of the 13,000 available (200 workers
× 65 seconds). Bounded producer slots filled, 22,494 offers were rejected by the
load generator before reaching the adapter, and submission timed out with some
commit outcomes unknown. This identifies the backlog/latency mechanism, not the
ultimate cause of slower commits.

There was no NATS adapter change between that failure and the passing repeats
below. No result here proves the adapter is defect-free or that every failure is
infrastructure-only.

## Repeats and matched worker comparison

- [Existing implementation, diagnostics off/on/on/off](https://github.com/Jitpomi/dogrs/actions/runs/37773937320):
  4/4 passed, each accepting/completing/verifying 54,000; 61.253, 60.587, 60.682,
  60.321 seconds. No adapter changes preceded these passes.
- [Repeat with hardware and host-I/O capture](https://github.com/Jitpomi/dogrs/actions/runs/37774414989):
  4/4 passed, 61.726, 60.823, 60.570, 60.730 seconds. The runner reported AMD
  EPYC 7763, four logical CPUs. Instrumented trials averaged 81–85 ms in batch
  execution, 12 ms in metadata batch queueing and 75–90 ms in enqueue batch
  queueing. Native host disk counters recorded approximately 24.6 GB written
  over each 70–71-second measurement/verification interval. These counters are
  host-wide and must not be attributed exclusively to NATS or to timed offers.
- [Matched workers 2/8/8/2 per tenant](https://github.com/Jitpomi/dogrs/actions/runs/37774519963):
  all four passed on one AMD EPYC 7763 runner, in 61.394, 60.272, 60.233 and
  60.516 seconds. Since both controls passed, this does not demonstrate a fix
  for the failing environment and does not justify changing default concurrency.

[Redis workers 1/4/4/1](https://github.com/Jitpomi/dogrs/actions/runs/37775422842)
also passed all four trials on AMD EPYC 7763. One-worker trials had enqueue p95
21–24 ms, versus 76–85 ms with four workers. More workers did not establish a
fix and worsened latency on this tested host; the default remains one.

## Separate ordinary matrix runs

[Run `57283fa`](https://github.com/Jitpomi/dogrs/actions/runs/37774529876):

| Backend | Runner CPU model | Accepted | Completed by deadline | Gate |
|---|---|---:|---:|---|
| PostgreSQL | AMD EPYC 7763 | 54,000 | 54,000 | Pass |
| NATS | AMD EPYC 7763 | 54,000 | 54,000 | Pass |
| Redis | Intel Xeon Platinum 8573C | 50,692 | 25,266 | Fail |

The Redis failure rejected 2,894 offers and reported one submission-deadline
error. Its Redis latency history had a median of 22 ms across the recorded
per-second `aof-fsync-always` maximum samples, versus 2 ms in the preceding
passing run. These are per-second event maxima, **not per-operation median
latency**. Script/command maxima remained 1–2 ms. This supports persistence
latency as a contributor, without isolating host hardware, contention or every
adapter scheduling cost.

[Run `3eda3be`](https://github.com/Jitpomi/dogrs/actions/runs/37775427381):

| Backend | Runner CPU model | Accepted | Completed by deadline | Gate |
|---|---|---:|---:|---|
| PostgreSQL | Intel Xeon Platinum 8370C | 54,000 | 54,000 | Pass |
| Redis | AMD EPYC 9V74 | 54,000 | 54,000 | Pass |
| NATS | Intel Xeon 6973P-C | 31,793 | 22,595 | Fail |

NATS rejected 21,612 offers and reported one submission-deadline error. CPU
model labels correlate with these observed environments; they do not establish
that the CPU itself caused persistence latency or that all Intel/AMD hosts have
these properties. The matrix assigns a separate host to each backend.

[Run `92ccc5a`](https://github.com/Jitpomi/dogrs/actions/runs/37775716907)
again passed Redis (AMD EPYC 7763), but PostgreSQL and NATS missed on separate
AMD EPYC 9V45 runners. PostgreSQL accepted/completed 53,492, rejecting 508 with
zero operation errors. NATS accepted 31,745 and completed 21,541, rejecting
21,314 with one submission-deadline error. This rules out a simple CPU-brand
classification; environment sensitivity is not a CPU-brand diagnosis.

[Run `42f186f`](https://github.com/Jitpomi/dogrs/actions/runs/37776468345)
passed Redis on AMD EPYC 7763. PostgreSQL missed on AMD EPYC 9V74 (46,063
accepted, 39,616 completed, 7,642 overload, one deadline error); NATS missed on
Intel Xeon Platinum 8573C (30,817 accepted, 20,676 completed, 22,473 overload,
one deadline error). Thus the ordinary matrix remains intermittent even though
the common-runner comparison passed all six cases. Do not describe all CI gates
as consistently green.

## ARM comparison

[Six cases on `42f186f`](https://github.com/Jitpomi/dogrs/actions/runs/37776461150)
used one Neoverse-N2 runner and the same default settings. This comparison failed:

| Order | Backend | Accepted | Completed by deadline | Gate |
|---|---|---:|---:|---|
| 1 | PostgreSQL | 54,000 | 54,000 | Pass |
| 2 | Redis | 54,000 | 50,934 | Fail |
| 3 | NATS | 47,567 | 34,590 | Fail |
| 4 | NATS | 49,163 | 41,993 | Fail |
| 5 | Redis | 54,000 | 54,000 | Pass |
| 6 | PostgreSQL | 54,000 | 54,000 | Pass |

The first NATS case had one submission-deadline error; the others had zero
operation errors. Later verification does not replace on-time completion.
ARM is an optional labelled comparison, not a proven replacement for the
existing x86 capacity environment, whose default remains unchanged.

### Reproduced ARM failure with matched workers and diagnostics

[Run `37777896271`](https://github.com/Jitpomi/dogrs/actions/runs/37777896271)
used Neoverse-N2, one binary and fresh fixtures in 2/8/8/2-worker order:

| Workers/tenant | Accepted | Completed by deadline | Overload | Errors |
|---:|---:|---:|---:|---:|
| 2 | 46,097 | 45,223 | 7,903 | 0 |
| 8 | 47,149 | 47,149 | 6,851 | 0 |
| 8 | 47,771 | 47,771 | 6,229 | 0 |
| 2 | 49,405 | 49,405 | 4,595 | 0 |

All four failed. Increasing workers can drain accepted work here, but did not
fix admission capacity; the final two-worker control accepted more than either
eight-worker case. Do not change defaults based on these results.

First-staging waits averaged 7.6–9.4 ms, while post-staging waits to the durable
acknowledgement averaged 101–118 ms. Enqueue batch execution averaged 112–130 ms;
waiting for that bounded lane averaged 1.11–1.25 seconds. The failure is not
explained by waiting only for initial frame admission or by too few consumer
workers. Slow commit completion and bounded execution create admission backlog.

Across 180 server stack snapshots, 1,283 sampled goroutine stacks contained
`syscall.Fsync`; 1,145 of those included `raft.storeToWAL`. Those are sampled
stack occurrences (the same goroutine can appear repeatedly), not distinct
fsync calls, CPU percentages or measured syscall durations. Host disk counters
recorded aggregate write-request waits of 5.5–9.2 ms, compared with 3.3–3.5 ms
in the passing instrumented x86 repeat; these are whole-interval host metrics,
not per-commit latency or a controlled architecture-only comparison.

Together these observations identify a concrete server persistence bottleneck
in a failing environment and the resulting adapter queueing. They do not prove
that all costs are infrastructure-only or establish optimal adapter design.
The original failed uninstrumented runner was not retained for replay, so the
exact contribution of its disk, scheduling and other costs cannot be recovered.

## Existing server attribution

The earlier [server execution trace](jetstream-admission-attribution-3d1dbd3.md#server-execution-trace-locates-a-concrete-persistence-cost)
identified substantial wait in NATS `raft.storeToWAL → fileStore.StoreMsg →
msgBlock.flushPendingMsgsLocked → os.File.Sync → syscall.Fsync`. That local
1000/s experiment measured 16–17 ms mean fsync durations and about 90–92% of
recorded syscall delay in fsync. These are concurrent wait totals, not CPU
percentages, and not a trace of this turn's failed GitHub runs. They support a
concrete persistence cost without establishing storage as the exclusive cause
of every miss. Larger batches and more concurrent execution were already tried
and rejected in the linked investigation; repeating speculative adapter changes
would not be justified by the current passing controls.

## Harness changes

Capacity artifacts now record CPU and filesystem/block-device details on Linux.
Diagnostic mode also samples read-only disk counters and CPU/I/O pressure once
per second, alongside existing server goroutine stacks and adapter timings.
The older fsync diagnostic reuses that sampler; it no longer injects a second
sampler into a generated controller.

A worker comparison preserves every failure and tests NATS 2/8/8/2 or Redis
1/4/4/1 on one runner with fresh fixtures. Defaults remain unchanged.

The `all-backends` mode builds once and tests PostgreSQL, Redis, NATS, then NATS,
Redis, PostgreSQL on the same runner. It preserves the baseline worker counts
(1, 1, 2), shard counts (1, 1, 16), rate, payload size and durability. Fresh
fixtures isolate stored state; reversing order helps expose order effects.
Every failed case remains a workflow failure, even if a later case passes.
Regression tests verify settings/order and failure preservation. This mode
complements rather than replaces the existing matrix gate.

## Correctness validation

[Full correctness/recovery CI on `42f186f`](https://github.com/Jitpomi/dogrs/actions/runs/37776468258)
passed all 11 jobs, including live backend checks and the five-minute Redis
recovery check. The local workflow/controller regression suite passed all 14
tests, and four existing diagnostic-result tests passed. No queue adapter code
was changed during this follow-up.
