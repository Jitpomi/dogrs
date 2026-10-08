# Redis duplex deadlock fix and repeated capacity validation

## Confirmed defect and fix

DogRS depended on `redis` 0.28.2. Its multiplexed driver's `poll_ready` did not
advance reads, and `poll_flush` awaited writer readiness before polling replies.
When both socket directions fill, this ordering can stop progress. This is the
upstream [redis-rs #1955 defect](https://github.com/redis-rs/redis-rs/issues/1955).
The [1.7.1 driver](https://docs.rs/redis/1.7.1/src/redis/aio/multiplexed_connection.rs.html)
advances both directions and includes an upstream regression.

Commit `fe35f7b` upgrades both dog-queue and the hosted-system example to 1.7.1.
The queue's 500 ms reconnect-delay cap, three retries, five-second connection
and ten-second response timeouts are preserved using the new typed API. No
producer semaphore or additional connection is introduced. Queue scripts,
public queue API, payload representation and durability requirements are unchanged.

`dog-queue/tests/redis_transport.rs` exercises the client's public API with a
256-byte duplex socket and sixteen concurrent 64 KiB ECHO requests/responses.
The test server stops reading while its reply write is blocked, forcing the
bidirectional pressure condition. The same test (only adjusting the connection
info accessor) fails on 0.28.2 after its five-second deadlock deadline and passes
on 1.7.1 in approximately 40 ms. This establishes a concrete dependency defect
and fix, independently of cloud storage performance. It does not prove every
historical capacity miss was caused by that defect.

## Same-runner comparisons

Each comparison uses one runner and one binary, four fresh fixtures, and
instrumentation off/on/on/off. All runs offer 900 jobs/second across 100 tenants,
unique 64 KiB payloads, 60 seconds of offers and five seconds of drain. Redis
uses one worker per tenant and AOF always-fsync; NATS uses two workers per tenant,
16 shards, R3 file storage and always-fsync. Automatic persistence remains enabled.

| Backend / client | Off trial 1 | On trial 2 | On trial 3 | Off trial 4 |
|---|---:|---:|---:|---:|
| Redis 0.28.2 completed | 34,010 | 49,258 | 42,961 | 2,983 |
| Redis 1.7.1 completed | 54,000 | 54,000 | 54,000 | 54,000 |
| NATS unchanged completed | 54,000 | 54,000 | 54,000 | 54,000 |

Sources: [old Redis](https://github.com/Jitpomi/dogrs/actions/runs/37759112395),
[upgraded Redis](https://github.com/Jitpomi/dogrs/actions/runs/37760804496),
[NATS](https://github.com/Jitpomi/dogrs/actions/runs/37759115901).
Each row uses a single runner, but the rows use different runners; do not treat
this as a hardware-matched old/new throughput measurement.

All four upgraded Redis runs accepted, completed and verified every offered job,
with zero overload, late offers or operation errors. Elapsed times were 60.067,
60.095, 60.088 and 60.048 seconds. All four NATS runs also accepted, completed and
verified every job without errors, overload or late offers. These are 216,000
verified jobs per backend across four separate one-minute workloads, not one
continuous soak. Instrumentation includes server-stack sampling for NATS.

The old Redis diagnostic trials measured peaks of 3,200 outstanding enqueue
writes. Its final ordinary trial recorded 4,585 errors, mostly timeouts, while
recorded server fsync peaked at 39 ms. This supports investigating client progress
instead of assuming every long wait is one slow server syscall.

## Validation and remaining limits

- Full [CI on the upgrade](https://github.com/Jitpomi/dogrs/actions/runs/37760801910)
  passed, including the new regression test.
- Local Redis cross-connection, deduplication/history and concurrent-claim
  contracts passed. The Redis/PostgreSQL-enabled queue suite also passed.
- Local SIGKILL/restart recovery with always-fsync passed with the upgraded binary.
- A local Mac Docker 900/s run still failed: 51,630 accepted and 13,971 completed
  within the deadline, with no operation errors. This failure remains evidence;
  the driver fix does not establish universal capacity across deployments.

Decision: retain the client upgrade and deterministic regression. The GitHub
configuration now has four-trial passes for both backends at the requested 900/s
target. Do not claim a universal throughput guarantee, a resolved 1,000/s target,
or that all infrastructure and overload questions are closed. Raw local regression,
contract and recovery logs are preserved under `redis-duplex-fix` in the task
workspace. The linked GitHub artifacts preserve the capacity evidence.
