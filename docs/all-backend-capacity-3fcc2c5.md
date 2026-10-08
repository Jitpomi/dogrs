# All-backend repeated capacity validation at 900 jobs/second

Tested commit: `3fcc2c5`, including the Redis 1.7.1 duplex-deadlock fix.
All three backends were tested afresh. Each backend used one assigned GitHub
runner for four fresh fixtures in diagnostic off/on/on/off order, with one
compiled binary throughout its four trials. Backends used separate runners.

Each trial offered 54,000 unique 64 KiB jobs: 100 tenants, nine jobs/second per
tenant, 60 seconds of arrivals and five seconds of drain. Redis used AOF
always-fsync; NATS used R3 file storage and always-fsync; PostgreSQL retained
fsync, synchronous commits and full-page writes. Worker counts remained one per
tenant for Redis/PostgreSQL and two for NATS. Other workflow defaults were
unchanged, including 16 NATS shards. Diagnostics include server-stack sampling
for NATS. This compares the full diagnostic mode, not just counter overhead.

| Backend | Trial | Diagnostics | Accepted | Completed by deadline | Verified later | Overload | Elapsed | Verdict |
|---|---:|---|---:|---:|---:|---:|---:|---|
| postgres | 1 | Off | 51,941 | 51,941 | 51,941 | 2,059 | 65.003 s | Fail |
| postgres | 2 | On | 54,000 | 54,000 | 54,000 | 0 | 60.864 s | Pass |
| postgres | 3 | On | 54,000 | 54,000 | 54,000 | 0 | 61.035 s | Pass |
| postgres | 4 | Off | 54,000 | 54,000 | 54,000 | 0 | 60.750 s | Pass |
| redis | 1 | Off | 54,000 | 43,952 | 43,971 | 0 | 65.002 s | Fail |
| redis | 2 | On | 54,000 | 51,380 | 51,409 | 0 | 65.006 s | Fail |
| redis | 3 | On | 54,000 | 50,787 | 50,814 | 0 | 65.001 s | Fail |
| redis | 4 | Off | 54,000 | 49,513 | 49,536 | 0 | 65.000 s | Fail |
| nats | 1 | Off | 54,000 | 54,000 | 54,000 | 0 | 61.362 s | Pass |
| nats | 2 | On | 54,000 | 54,000 | 54,000 | 0 | 60.751 s | Pass |
| nats | 3 | On | 54,000 | 54,000 | 54,000 | 0 | 60.283 s | Pass |
| nats | 4 | Off | 54,000 | 54,000 | 54,000 | 0 | 60.414 s | Pass |

All twelve trials reported zero operation errors and zero late offers. A pass
still requires every offer to be accepted, completed within the deadline and
verified; zero errors alone is insufficient. Late verification counts cannot
replace on-time completion counts.

## Outcome

- NATS: **4/4 passed**, all 216,000 jobs verified.
- PostgreSQL: **3/4 passed**. The first trial rejected 2,059 offers under load;
  all accepted jobs completed and verified. Overall comparison failed.
- Redis: **0/4 passed**. Every trial admitted all 54,000 jobs, but each left a
  completion backlog at the deadline. Overall comparison failed.

Therefore the suite does **not** establish that all backends reliably meet 900/s
in these configurations. The fixed Redis transport deadlock is independently
proven by its regression test; these capacity failures do not invalidate that
fix, and that fix does not excuse the failures. Prior passes remain historical
evidence, not substitutes for these results. No provider-failure or hours-long
soak certification follows from these twelve fresh one-minute workloads.

## Sources

- [postgres comparison](https://github.com/Jitpomi/dogrs/actions/runs/37762093043)
- [redis comparison](https://github.com/Jitpomi/dogrs/actions/runs/37762097108)
- [nats comparison](https://github.com/Jitpomi/dogrs/actions/runs/37762100342)

Artifacts include all trial logs, settings, diagnostic metrics and binary hashes.
