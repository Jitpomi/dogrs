# Capacity reliability investigation — 2026-10-25

Status: **18/18 passed at 900/s on the Mac VM with `compact-4`; 18/18 previously passed on the AMD EPYC 7763 x64 runner with the original layout**. This is an environment-specific workload qualification, not a guarantee for every runner or production deployment. Failures from the earlier ARM and original-layout Mac runs below remain valid.

All measurements below offer 54,000 unique 64 KiB jobs across 100 tenants over 60 seconds, with the existing five-second drain and 100 ms late-offer threshold. PostgreSQL durable commits, Redis always-fsync, and NATS three file-backed always-fsync replicas remain enabled.

The local environment was the eight-vCPU/16 GiB Lima VM on an actively used Mac. Host CPU contention and approximately 98% disk utilisation were observed. Docker Desktop was stopped before the `qualification-quiet` and subsequent experiments, but this does not make the Mac a physically dedicated host. The original queue implementation is retained: longer enqueue collection windows did not establish reliability.

| Experiment / fixture | Accepted | Completed | Late offers | Overload | Errors | Passed |
|---|---:|---:|---:|---:|---:|---|
| capacity-diag/dogrs-fault-8974c5d28f | 54000 | 54000 | 127 | 0 | 0 | False |
| capacity-window-control/dogrs-fault-1b62a134a7 | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity-window-candidate/dogrs-fault-ab6f21ab8b | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity-window-candidate/dogrs-fault-1b1f25c245 | 54000 | 54000 | 61 | 0 | 0 | False |
| capacity-window-control/dogrs-fault-f0bbd7a886 | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity20-candidate20/dogrs-fault-8c0fc6c045 | 53968 | 53968 | 2 | 32 | 0 | False |
| capacity20-control/dogrs-fault-b88fb8b52b | 54000 | 54000 | 0 | 0 | 0 | True |
| capacity20-control/dogrs-fault-1e44479966 | 35370 | 35359 | 13717 | 18402 | 501 | False |
| capacity20-candidate20/dogrs-fault-d4952a0eca | 52423 | 51272 | 762 | 1577 | 0 | False |
| qualification-quiet/round-1/dogrs-fault-4a39eb7924 | 54000 | 54000 | 0 | 0 | 0 | True |
| qualification-quiet/round-1/dogrs-fault-723051cc20 | 54000 | 54000 | 0 | 0 | 0 | True |
| qualification-quiet/round-1/dogrs-fault-c2505c48e5 | 51043 | 50981 | 4869 | 2917 | 142 | False |
| qualification-quiet/round-1/dogrs-fault-ee602b48ba | 47532 | 47482 | 6816 | 6119 | 647 | False |
| capacity-runtime-2/dogrs-fault-2752dbfbfc | 54000 | 54000 | 144 | 0 | 0 | False |
| capacity-runtime-8/dogrs-fault-c78f1c47e0 | 51054 | 51054 | 3599 | 2946 | 0 | False |
| capacity-runtime-8/dogrs-fault-9dcbdf14d2 | 50907 | 50862 | 2443 | 3093 | 0 | False |
| capacity-runtime-2/dogrs-fault-8be4b2799a | 51947 | 51947 | 2692 | 2053 | 0 | False |
| capacity-shards-8/dogrs-fault-0700140d7a | 54000 | 54000 | 27 | 0 | 0 | False |
| capacity-shards-16/dogrs-fault-5311a2cd57 | 49510 | 42147 | 103 | 3455 | 1 | False |
| capacity-shards-16/dogrs-fault-9e1bd5dd21 | 54000 | 54000 | 71 | 0 | 1 | False |
| capacity-shards-8/dogrs-fault-399ea332a2 | 39051 | 38870 | 325 | 14597 | 1 | False |

`capacity-diag` was instrumented and used a different tenant prefix; it is diagnostic evidence, not a matched capacity verdict. Other local comparisons use `dogrs-test-all-backends`. Runtime comparisons explicitly set GOMAXPROCS per NATS server; shard comparisons use fresh fixtures and never reshard existing data.

The partial `qualification-quiet` attempt was stopped after both NATS cases failed, before the remaining fourteen cases. It is a failed/incomplete qualification, not an 18-case result. All output was preserved.

The reusable qualification mode requires all eighteen measurements to pass and retains every failure. Regression checks reject reduced rate/duration and profiling. The six-case isolated ARM diagnostic with eight NATS shards is [run 40222520019](https://github.com/Jitpomi/dogrs/actions/runs/40222520019); its result must be reviewed before adopting any topology change.

Bounded late-offer samples and maximum lateness are now recorded to distinguish startup delays from mid-run stalls. They do not change scheduling, admission, timing limits, or pass/fail logic.

## Isolated ARM results

[Run 40222520019](https://github.com/Jitpomi/dogrs/actions/runs/40222520019)
used a fresh GitHub ARM runner and eight NATS shards. PostgreSQL and Redis each
passed both trials: all 54,000 admitted, completed and verified, with zero late
offers, overload or errors. NATS admitted/completed 49,288 and 49,014, rejecting
4,712 and 4,986 offers respectively. Neither NATS trial had late offers or
operation errors. Eight shards therefore did not qualify.

The separately instrumented timing report in
[run 40222817094](https://github.com/Jitpomi/dogrs/actions/runs/40222817094)
used sixteen NATS shards on four Neoverse-N2 CPUs. It recorded zero late offers
(maximum offer lateness 20.231 ms), 41,928 accepted, 28,609 completed, 11,805
overload rejections, and a submission-deadline error. This distinguishes the
remaining sustained-throughput bottleneck from the local Mac's timing stalls.
A bounded 1 MiB enqueue batch target is under evaluation in
[run 40223255704](https://github.com/Jitpomi/dogrs/actions/runs/40223255704),
with two/eight/eight/two workers per tenant. This is not yet an accepted fix.

The larger-batch comparison failed all four cases. With eight workers, admitted
and completed counts were 48,362 and 48,064; neither met 54,000. With two workers,
completion was only 14,899 and 18,376. No late offers occurred. The 1 MiB target
was rejected and reverted to the original 256 KiB target. Runtime concurrency,
shard count, collection windows, and workload thresholds are not changed by
these rejected experiments.

The next qualification uses an isolated x64 job and the original queue settings,
with all eighteen measurements required. This explicitly evaluates a different
hardware class rather than treating the failing ARM cases as passed. The actual
CPU, toolchain and environment must accompany any resulting claim.

The combined eight-shard / 1 MiB batch experiment in
[run 40224916592](https://github.com/Jitpomi/dogrs/actions/runs/40224916592)
also failed all four cases. Eight-worker trials admitted/completed 49,542 and
50,109 jobs. Two-worker trials completed only 17,168 and 20,019. No late offers
occurred. The experimental branch was removed after retaining its artifacts;
none of these changes is adopted as a reliability fix.

## Completed x64 qualification

[Run 40224103288](https://github.com/Jitpomi/dogrs/actions/runs/40224103288)
completed all eighteen predeclared measurements without retries: three rounds of
PostgreSQL, Redis, NATS, NATS, Redis, PostgreSQL. Source commit was
`6a5c7e6722352b3cb7ad2418ee048c70b6a8e697`, Rust 1.99.1, x86_64 Linux,
four logical CPUs on AMD EPYC 7763. The original queue settings were retained.

| Backend | Passed | Accepted / completed / verified | Elapsed range |
|---|---:|---|---|
| PostgreSQL | 6/6 | 54,000 / 54,000 / 54,000 each | 60.063–60.084 s |
| Redis | 6/6 | 54,000 / 54,000 / 54,000 each | 60.048–60.096 s |
| NATS | 6/6 | 54,000 / 54,000 / 54,000 each | 60.204–61.618 s |

Total: 972,000 unique jobs. Every measurement had zero late offers, overload
rejections and operation errors. Rate, payload size, drain deadline and provider
durability settings were not relaxed. The independent report verifier at
`a0a2dc6` was run against the downloaded artifact and returned six measurements
per backend, `passed: true`, and no errors. That verifier is now integrated into
future qualification runs; this run began before its integration.

The result measures queue persistence and payload integrity, not HTTP throughput,
payment processing, power-loss recovery or a multi-host production cluster.
Provider fsync settings were checked, but the hosted runner's recorded root
filesystem uses `nobarrier` and `data=writeback`; this must not be presented as
proof of storage durability under host/power failure. NATS replicas share this
one host. The result also does not establish low request latency: PostgreSQL
enqueue p95 ranged from 1.151 to 1.301 seconds. No latency SLO beyond the declared
capacity criteria was evaluated.

The supported conclusion is repeated 900/s capacity on this recorded environment.
`ubuntu-latest` does not guarantee this CPU on later allocations. Requalify a
production host and storage configuration before promising that deployment's
capacity. The failed ARM tuning experiments were reverted, not hidden.

Artifacts are retained in the run and locally under
`artifacts/dogrs-capacity-reliability-2026-10-25/x64-qualification-40224103288`
in the task workspace. Docker Desktop was restored; the temporary Lima capacity
VM is stopped and no continuously listening runner was installed.

## Mac VM follow-up

The Mac VM remains subject to its host's CPU and storage contention. A follow-up
comparison used the same release binary, eight-vCPU/16 GiB VM, sixteen NATS
shards, two workers per tenant, and unchanged 900/s workload. Docker Desktop was
stopped for this session. Ordinary rootful Docker was tested against the existing
rootless Docker in rootful/rootless/rootless/rootful order. Both rootless cases
passed; rootful passed one and failed one. The first rootful case accepted 50,434,
completed 45,102, recorded 3,279 overload rejections, 55 late offers and one
submission-deadline error. Host load varied, so this does not establish a causal
networking advantage. No networking migration or queue tuning was adopted.

A fresh eighteen-measurement Mac qualification began at 19:46 UTC on October 25
with the original rootless setup, Docker Desktop stopped, AC power and sleep
inhibited. All cases run even after a failure. The complete result was PostgreSQL 6/6, Redis 6/6, NATS 2/6. The independent
verifier rejected all four failing NATS measurements. This Mac qualification
failed; earlier successful diagnostic NATS trials do not override that result.

The first Mac follow-up NATS failure admitted/completed 53,212 of 54,000 jobs,
with 788 overload rejections and 393 late offers. Its retained stream snapshots
show all expected jobs on the four-to-six-tenant shards, while the two busiest
(eight-tenant) shards accepted 4,085 and 4,105 of 4,320 expected jobs. This motivates
a fresh-topology diagnostic: four shards with eight enqueue and four metadata
lanes per shard, instead of sixteen shards with two enqueue and one metadata
lane. Both configurations have 32 enqueue and 16 metadata execution lanes in
total. This is only a hypothesis until tested; never change the shard count of
an existing ledger without a migration.

Both compact-layout trials and both original-layout controls passed in the
four/sixteen/sixteen/four comparison. This alone does not establish that the
layout resolves intermittent failures. A new complete eighteen-case Mac
qualification started at 20:13 UTC using the explicit `compact-4` layout.
The complete run passed all eighteen measurements. The independent verifier checks the four-stream topology,
eight enqueue/four metadata lanes, and the unchanged workload and durability
criteria; selecting this profile does not accept a sixteen-stream result.

## Verified Mac VM qualification

The `compact-4` run completed at approximately 20:33 UTC on October 25 (14:33
America/Denver), with no retries. PostgreSQL, Redis and NATS each passed all six
measurements: 972,000 jobs accepted, completed and verified exactly once, zero
late offers, overload rejections or operation errors. The independent verifier
returned `passed: true`, six cases per backend, and no errors.

| Backend | Passed | Elapsed range | Jobs accepted/completed/verified per case |
|---|---:|---:|---|
| PostgreSQL | 6/6 | 60.050–60.169 s | 54,000 / 54,000 / 54,000 |
| Redis | 6/6 | 60.052–60.068 s | 54,000 / 54,000 / 54,000 |
| NATS | 6/6 | 60.133–64.936 s | 54,000 / 54,000 / 54,000 |

This was a local Mac VM run, not a GitHub-hosted capacity job. The host was Apple
M1 Max with 64 GiB physical memory; the existing VM retained eight virtual CPUs,
16 GiB RAM and rootless Docker. Docker Desktop was stopped, AC power connected,
and sleep inhibited. Other user applications were not forcibly paused or closed.
Source was `6ff9053` and Rust 1.99.1. No queue algorithm, replication count, fsync
policy, payload, offered rate, admission bound or drain deadline was weakened.
Only the NATS fixture topology and per-shard write lanes changed: 16×(2 enqueue,
1 metadata) became 4×(8 enqueue, 4 metadata), retaining 32/16 total lanes.

The dedicated Mac workflow now defaults to the tested `compact-4` profile.
`original-16` remains available for explicit comparisons; hosted diagnostics keep
their previous defaults. Existing ledgers must not be resharded in place. This
qualifies the measured workload and environment, not every future host load or
power-loss recovery scenario. One NATS case used almost the entire five-second
drain allowance, and NATS enqueue p95 reached 3.596 seconds; no spare capacity or
low-latency SLO is claimed. All prior failures are retained.

The machine-readable [result summary](evidence/mac-capacity-2026-10-25.json)
records every measurement, source/verifier commits, binary identity and archive
checksum. Full raw reports and host-load samples are in the task workspace at
`artifacts/dogrs-mac-capacity-2026-10-25/`; the independent verification command is:

```sh
python3 .github/scripts/verify_capacity_reports.py   /path/to/mac-qualification-compact4-20261025 --nats-layout compact-4
```

The workflow/verifier suite passed 25 tests. Full CI for the verifier/profile
implementation passed in [run 40229822947](https://github.com/Jitpomi/dogrs/actions/runs/40229822947).
The final default selection and evidence documentation followed that run.
