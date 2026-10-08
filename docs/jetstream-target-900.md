# 900 jobs/second target

At the user's request, ordinary aggregate capacity runs and the Provider capacity
workflow now default to nine jobs/second per tenant across 100 tenants (900/s).
Explicit `--rate 10` retains the 1,000/s stress workload. Attribution diagnostics
retain their fixed ten-per-tenant default. This is a target change, not a capacity
fix or a passing certification. The adapter and durability settings are unchanged.

Three fresh local NATS 2.15.0 R3, file-backed, always-fsync fixtures were tested
with 16 shards, four workers per tenant, 32 in-flight offers per tenant, a shared
connection, default atomic writer settings and unique 64 KiB payloads. Diagnostics
were disabled. Each run offered 54,000 jobs over 60 seconds with five seconds to
drain, on the same Apple M1 Max host used for the 500/s baseline.

| Run | Offered | Accepted / completed / verified | Overload rejections | Elapsed | Verdict |
|---:|---:|---:|---:|---:|---|
| 1 | 54,000 | 46,863 | 7,137 | 65.009 s | Fail |
| 2 | 54,000 | 50,680 | 3,320 | 65.004 s | Fail |
| 3 | 54,000 | 46,295 | 7,705 | 65.011 s | Fail |

All runs had zero operation errors and late offers, but rejected offered work
under load. Therefore 900/s remains unmet on this fixture. The [500/s baseline](jetstream-operating-rate-500.md)
remains the passing evidence; its historical results are not reclassified.

Measurements used the previously built configurable-rate release binary with
explicit `--rate 9`; only the default selection changed afterward in source.
Binary SHA-256: `0230b8b59d2a3cfa0933cabec6c067c276bb91e4307ecd30b46254af91be7ab3`.
Raw reports and runner are in the task workspace under `dogrs-target-900`.
No paid resources were created; disposable fixtures were removed.
