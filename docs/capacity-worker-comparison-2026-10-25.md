# Capacity worker comparisons — 25 October 2026

These results do not establish stable capacity across hosted runners. The target
remains 900 jobs/second across 100 tenants, 64 KiB payloads, 60 seconds of offers
and five seconds of drain. Durability settings are unchanged.

## Latest ordinary matrix

[Run 40213271955](https://github.com/Jitpomi/dogrs/actions/runs/40213271955):

| Backend | Accepted | Completed | Rejected under load | Operation errors | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| PostgreSQL | 52,058 | 52,058 | 1,942 | 0 | Fail |
| Redis | 54,000 | 54,000 | 0 | 0 | Pass |
| NATS | 54,000 | 54,000 | 0 | 0 | Pass |

An earlier matrix on 7c6a7e3 failed all three:
[40212411438](https://github.com/Jitpomi/dogrs/actions/runs/40212411438).
The subsequent a2f5310 change only enabled PostgreSQL in the manual worker
comparison workflow; it did not change queue implementation or ordinary defaults.
Do not attribute the new passes to a queue performance fix.

## Controlled comparisons

Each backend used one runner and repeated settings in low/high/high/low order.
Different backends used different runners. No rate, deadline, payload or durability
relaxation was applied. Counts below group repetitions by setting rather than
implying an order within a pair.

| Backend | Workers per tenant | Completed in each repetition | Gate |
| --- | ---: | --- | --- |
| NATS | 2 | 54,000 / 54,000 | Both pass |
| NATS | 8 | 54,000 / 54,000 | Both pass |
| Redis | 1 | 54,000 / 54,000 | Both pass |
| Redis | 4 | 51,352 / 53,821 | Both fail |
| PostgreSQL | 1 | 52,399 / 51,320 | Both fail |
| PostgreSQL | 4 | 51,647 / 51,193 | Both fail |

Sources: [NATS](https://github.com/Jitpomi/dogrs/actions/runs/40213249749),
[Redis](https://github.com/Jitpomi/dogrs/actions/runs/40213247780),
[PostgreSQL](https://github.com/Jitpomi/dogrs/actions/runs/40213277723).

All comparison cases recorded zero operation errors. Redis accepted all 54,000
jobs in every case, but higher concurrency missed the completion deadline.
PostgreSQL completed every accepted job in every case; admission rejected the
remainder. More consumer concurrency did not solve its admission bottleneck.
NATS passed all four cases on this runner, so this experiment cannot establish
that eight workers fixes its failures on other runners.

## Decision

Keep ordinary worker defaults unchanged. Increasing all worker counts is not
supported by this evidence and made Redis worse in this comparison. PostgreSQL
needs enqueue/pool/storage attribution; a capacity miss alone does not prove an
infrastructure-only cause. Investigate with the existing stage-timing and database
wait diagnostics before changing the adapter or deployment defaults.

The separate correctness/recovery CI on 7c6a7e3 passed:
[40212411372](https://github.com/Jitpomi/dogrs/actions/runs/40212411372).
That verifies the file-journal fix and other correctness checks, not the separate
sustained-capacity target.


## PostgreSQL stage attribution

[Instrumented run 40214506344](https://github.com/Jitpomi/dogrs/actions/runs/40214506344)
accepted, completed and verified 48,695 of 54,000 offers; the load generator
rejected 5,305 while its in-flight budgets were full. Operation errors and late
offers were zero. This is a capacity failure, not a passing recovery run.

Mean enqueue-slot wait was 1,639.212 ms, compared with 0.040 ms for pool
acquisition and 49.811 ms for the INSERT request. The pool itself was not the
observed bottleneck. The server recorded 9.094 ms mean INSERT execution time;
that statistic and client request duration cover different work and should not
be subtracted to claim an exact commit latency.

Of sampled enqueue backend observations, 79.6% waited on `LWLock/WALWrite`
and 14.0% on relation extension. These are counts weighted by the number of
backends in each sample, not percentages of elapsed application time. Observed
extension waits included the TOAST relation. PostgreSQL I/O statistics recorded
18.629 aggregate seconds of WAL initialization fsync time and 32.962 seconds
of normal client-backend WAL fsync time. This supports investigating write and
commit contention; it does not establish a single hardware-only cause.

The next controlled comparison uses producer caps 16/48/48/16 on one runner,
with stage/database profiling disabled. The ordinary cap remains 48. No target,
drain deadline, payload, pool size or durability setting is relaxed. Earlier
admission batching was already rejected by a matched comparison; see
[the earlier investigation](redis-postgres-capacity-follow-up.md).
