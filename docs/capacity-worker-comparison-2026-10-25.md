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
