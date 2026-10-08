# Redis stall attribution

The 1,000/s capacity failure on GitHub run `37727001717` admitted 52,411 of
60,000 offers and completed 47,562 within 65 seconds. Enqueue p95 was 4.61 s and
maximum acknowledgement latency was 10.24 s. Run `37722742793` had passed the same
rate, payload size and worker count. Redis adapter code had not changed between
them. Both performed automatic AOF rewrites; the failed run had one more rewrite.
That correlation does not establish rewrite activity as the exclusive cause.

The fixture now enables Redis latency monitoring at 1 ms and saves event maxima
and timestamped histories after the workload. It retains always-fsync and the
existing rewrite policy. It does not save SLOWLOG arguments or job payloads.
See Redis's [latency monitor documentation](https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/latency-monitor/).
Command execution, AOF write, fsync and fork events are distinct observations;
client latency includes queueing and transport as well.

## Local controls and rejected candidates

Four fresh local fixtures ran original–bounded–bounded–original at 100 tenants,
ten jobs/second each, unique 64 KiB payloads, one worker per tenant, 32 in-flight
offers per tenant, 60 seconds plus five seconds of drain. All used Redis 7.4,
AOF always-fsync, automatic rewrites and the same latency monitor. The candidate
limited enqueues to 32 in flight per backend instance, leaving claims/completions
outside the semaphore. No queue semantics or atomic scripts changed.

| Layout | Accepted | Completed by deadline | Operation errors | Max acknowledgement |
|---|---:|---:|---:|---:|
| Original | 54,920 | 11,890 | 0 | 8.804 s |
| Bounded | 43,328 | 34,142 | 1 | 3.223 s |
| Bounded | 46,286 | 36,725 | 0 | 2.129 s |
| Original | 54,995 | 18,791 | 0 | 8.922 s |

Both bounded runs increased completion but reduced admission; all failed.
The first bounded run also had 308 late offers. Its error was an expired
submission deadline with uncertain outcomes. Maximum recorded server fsync waits
in the first three runs were 583, 507 and 466 ms respectively. Those measurements
cannot by themselves explain the full multi-second client waits.

A further single candidate used separate producer/consumer connections plus the
same enqueue limit. It admitted 40,072 and completed 39,922 jobs, with maximum
acknowledgement latency 1.564 s. It still failed, with a submission deadline error.
This trial is not a matched repeated proof of the separate-connection effect.

Neither candidate was retained. They establish sensitivity to producer queueing
and competing operations on this local fixture, not a 1,000/s fix or a complete
explanation for the GitHub failure. Local results must not be substituted for
GitHub runner measurements. Patches and raw evidence remain in the task workspace
under `redis-bounded-comparison`, `redis-isolated-producers` and
`redis-stall-attribution`. GitHub measurements with the new server events are recorded below.

## GitHub results

The original adapter failed run [37729496248](https://github.com/Jitpomi/dogrs/actions/runs/37729496248):
51,076 accepted, 44,084 completed, maximum acknowledgement 10.389 s. Server
maximum fsync was 208 ms, fork 35 ms and recorded command events 1 ms. This does
not support attributing the whole client delay to a single long fsync or fork.
It does not rule out cumulative persistence cost, scheduling, network delay or
client/server queueing.

The bounded, separate-connection candidate was temporarily pushed as `c2caef7`
for real GitHub validation, then reverted after the following mixed results:

| Run | Offered rate | Workers/tenant | Accepted | Completed | Verdict |
|---|---:|---:|---:|---:|---|
| [37729862773](https://github.com/Jitpomi/dogrs/actions/runs/37729862773) | 1,000/s | 1 | 60,000 | 60,000 | Pass |
| [37730338815](https://github.com/Jitpomi/dogrs/actions/runs/37730338815) | 1,000/s | 1 | 52,541 | 52,404 | Fail |
| [37730373245](https://github.com/Jitpomi/dogrs/actions/runs/37730373245) | 1,000/s | 4 | 54,000 | 54,000 | Fail |
| [37729853805](https://github.com/Jitpomi/dogrs/actions/runs/37729853805) | 900/s | 1 | 42,790 | 42,661 | Fail |

The passing run completed and verified every job in 62.516 s without errors,
overload or late offers. Its maximum acknowledgement was 117 ms, but maximum
server fsync was also much faster at 6 ms, so cross-run improvement cannot be
attributed solely to the code. The one-worker repeat had an expired submission
deadline; the four-worker run had zero operation errors but 6,000 overload
rejections. The 900/s run had an expired submission deadline too.

Live Redis cross-connection contract, deduplication/history, concurrent-claim
and SIGKILL/restart recovery checks passed for the candidate. Correctness passing
was insufficient to justify promoting an inconsistent capacity change.

Decision: retain diagnostic capture; restore the original adapter. The
hard-coded admission limit trades producer throughput for completion progress.
It is not a demonstrated solution to the requested capacity target. Future
comparisons need matched runner resources and explicit producer/consumer queue
measurements before choosing adaptive limits or a different batching strategy.
