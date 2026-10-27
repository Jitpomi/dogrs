# Capacity reliability investigation — 2026-10-25

Status: **The stricter Mac headroom qualification passed all 18 fresh runs: six each for PostgreSQL, Redis and NATS at 900 jobs/s.** The independent verifier passed every fixture with the stricter 62-second elapsed and 1,000 ms enqueue-p95 limits, zero late offers/dispatches, and all 54,000 jobs verified exactly once. The IDE and user build remained running. See the final qualification below and [machine-readable evidence](evidence/mac-capacity-6g-batch16-2026-10-26.json).

This qualifies the documented workload and Mac VM configuration; it does not establish a higher throughput ceiling or guarantee arbitrary host contention. Earlier failed experiments below remain valid historical evidence.

All measurements below offer 54,000 unique 64 KiB jobs across 100 tenants over 60 seconds, with the existing five-second drain and 100 ms late-offer threshold. PostgreSQL durable commits, Redis always-fsync, and NATS three file-backed always-fsync replicas remain enabled.

The initial local environment was the eight-vCPU/16 GiB Lima VM on an actively used Mac. Host CPU contention and approximately 98% disk utilisation were observed. Docker Desktop was stopped before the `qualification-quiet` and subsequent experiments, but this does not make the Mac a physically dedicated host. Those early collection-window experiments did not establish reliability on their own. The final configuration and retained changes are documented below.

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


## Headroom follow-up (2026-10-25, 20:41–20:46 UTC)

The dedicated qualification now additionally requires every measurement to finish
within 62 seconds and enqueue p95 to remain at or below 1000 ms. These targets
were chosen before the new experiments. Use `verify_capacity_reports.py --headroom`
with the matching layout. The previous compact-4 qualification fails this stricter
check in two NATS fixtures (7d164e2c06 and d023effacf). Its original pass remains
valid only against the original 65-second gate.

A runtime comparison (GOMAXPROCS=4 and TOKIO_WORKER_THREADS=4 versus automatic)
was stopped because the Mac exhausted available disk space. The first bounded
fixture c103971e9c accepted 31,993 and completed 22,505 jobs; the first automatic
control fccf4180ba accepted 42,760 and completed 26,217. Both failed. A second
automatic fixture 33f38b5ced was interrupted by host storage exhaustion and is not
a qualification result. The fourth planned measurement was not started. Reports
remain inside the stopped VM under ~/headroom-threads-{bounded,automatic}.
No runtime tuning from this experiment was adopted.

During these runs the host reported 17.6 GiB swap usage, substantial IDE background
CPU activity, and available space fell from 6.7 GiB to 162 MiB. Guest I/O pressure
was elevated and guest commands ultimately returned input/output errors. These
observations do not isolate the cause of the earlier narrow pass. The VM was shut
down to stop further writes. Docker Desktop remains stopped deliberately until
host disk space is recovered. Before resuming, recover sufficient host storage,
check the guest filesystem and disposable fixtures, and repeat controlled tests.
Headroom is **not yet qualified**; do not treat the new verifier as a performance fix.


### Safe cleanup and resumed measurements

The follow-up recovered host storage by deleting regenerable npm package cache,
Chrome HTTP/code/image cache directories (not Storage, profiles, cookies or
history), Xcode DerivedData, and three old Sparkle updater staging directories.
No project source, personal documents, credentials or arbitrary Docker volumes
were removed. Boot-time filesystem checking repaired the test VM; tune2fs then
reported clean. The release binary SHA256 still matched the earlier qualification.
Only the three abandoned containers for fixture 33f38b5ced and its isolated
network were removed. Guest fstrim reclaimed unused sparse-disk blocks. Host free
space rose from 2.1 GiB before cleanup to approximately 32 GiB afterward.

The unchanged compact-4 baseline then passed two 900/s NATS measurements:
39b8afbb63 finished in 60.0967 s with enqueue p95 117.11 ms; 49624256fd finished
in 60.0892 s with enqueue p95 230.27 ms. Both verified all 54,000 jobs, with zero
late offers, overload or errors. These two runs alone are not qualification.

Two additional 1000/s stress measurements each completed and verified 60,000
jobs, but failed the zero-late-offer requirement: 50a6cede5d had eight late offers
and 1066.93 ms enqueue p95; 455ba4145f had 136 late offers and 842.13 ms p95.
Neither is a pass or evidence of qualified 1000/s capacity. No runtime code or
thread tuning was adopted. A fresh eighteen-case 900/s headroom qualification
was started after these diagnostics; its results are recorded separately below.


### Follow-up outcome: headroom remains unqualified

The resumed qualification stopped after five completed fixtures: PostgreSQL and
both Redis cases passed, as did the first NATS case. The second NATS case,
dc81e793c5, completed and verified all jobs but recorded 136 late offers and
1845.75 ms enqueue p95. The sequence is failed and incomplete, not 18/18.
A host-space preflight paused between the first and second cases while macOS
reclaimed trimmed blocks; the same sequence resumed without repeating a case.

An 8 GiB guest-memory trial retained eight CPUs and the existing four buckets.
Both measurements completed all jobs but recorded 17 and 157 late offers.
A subsequent two/four/four/two-bucket comparison retained 32 total enqueue lanes
and 16 total metadata lanes: each layout passed once and failed once. These
trials do not justify adopting either change. The VM was returned to 16 GiB and
stopped after testing. Docker Desktop was restarted.

Late offers remained present even in a run with 74.2 ms enqueue p95 and 60.22 s
completion. Concurrent host builds and substantial swap usage were observed;
these observations suggest host scheduling interference but do not establish
exclusive causality. A quiet, controlled host interval is needed to isolate
remaining provider/runtime effects. No passing criterion was relaxed, and no
runtime tuning was adopted. The requested headroom improvement remains open.

Machine-readable results and raw-log checksums are in
[evidence/mac-headroom-followup-2026-10-25.json](evidence/mac-headroom-followup-2026-10-25.json).
The local archive is `artifacts/dogrs-mac-capacity-2026-10-25/headroom-followup.tar.gz`
in the parent task workspace, retaining failures and incomplete fixtures.


### Further diagnosis: server OOM and scheduler isolation

The subsequent 8 GiB experiment is rejected. Linux recorded global OOM kills of
`nats-server` at 23:06:11, 23:10:01 and 23:18:15 UTC. The last timestamp matches
fixture `c3182d7bea` losing its first node during terminal verification. Its
snapshot timeout was a server failure, not evidence that terminal records were
missing. The fixture had also missed the lateness and 62-second headroom gates.
The VM was returned to 16 GiB for comparison. A later 8 GiB experiment applies an explicit per-server Go memory budget; neither size is yet a qualified headroom profile.

Capacity fixtures now save each container's state before cleanup. The verifier
rejects OOM kills, exited servers and missing state evidence when the fixture
advertises this evidence format. It never serializes container credentials.

Two explicit experimental controls are available, with defaults unchanged:
`DOGRS_NATS_LEADER_PLACEMENT=balanced` distributes the four fixture stream leaders
2/1/1 across the three servers before timing; `DOGRS_CAPACITY_PRODUCER_RUNTIME=isolated`
drives the same open-loop workload on a separate current-thread Tokio runtime.
That runtime stays alive through terminal verification to preserve lazily spawned
provider tasks. Rate, payload, admission limit, absolute deadlines and late-offer
checks remain unchanged. Verification requires the chosen runtime and leader
profile to match the recorded evidence; these results cannot silently substitute
for the shared-runtime, naturally elected baseline.

The 16 GiB diagnostic fixture `c52b5cb9de` retained all three servers and verified
54,000 jobs in 60.357 seconds with enqueue p95 367.12 ms, but had 52 late offers.
It therefore failed, and its instrumentation also excludes it from qualification.
No new headroom qualification is claimed from these diagnostics.


The next 8 GiB profile sets `DOGRS_NATS_GOMEMLIMIT_MIB=1536` for each
NATS process. This is a Go soft memory budget, not a container hard limit.
Three initial NATS measurements passed without server death, including two in
`headroom-qualification-budget8`. That sequence failed at PostgreSQL fixture
`540875778d`: 51,285 admitted/completed, 2,715 overload rejections, 1,730 late
offers, 65.012 seconds, and enqueue p95 4,715.69 ms. Its already-started next
PostgreSQL fixture `4e75f84375` also failed (287 late offers, 62.490 seconds).
Those failures remain part of the evidence.

A diagnostic PostgreSQL run with 16 concurrent inserts retained all 54,000 jobs
and enqueue p95 80.04 ms, but recorded 44 late offers and is a failure. Sampling
showed most observed active enqueue waits on WAL writes or sync. The following
uninstrumented trial `3f5e44c4e1` passed in 60.047 seconds with enqueue p95
48.57 ms and maximum offer lateness 78.05 ms. This motivates a separate fixed
18-case sequence with the explicit 16-insert limit; it does not establish
reliability by itself. RustRover and the separate macOS build are left running
at the user's request. Docker Desktop is temporarily stopped.

Only regenerable package/build caches and unused registry-pulled Docker images
were removed to recover host disk space. Existing containers, volumes, projects
and personal files were preserved. The image-removal manifest is retained with
the local evidence. Disk reclamation is not claimed as a demonstrated cause of
any performance improvement.


The 16-insert qualification attempt stopped on its third fixture, NATS
`5d91478f59`: all 54,000 jobs verified in 61.090 seconds with enqueue p95
176.79 ms, healthy servers, but 35 late offers (maximum 128.487 ms). It failed.

A further opt-in `DOGRS_CAPACITY_PRODUCER_RUNTIME=paced` profile isolates only
the offer timers and dispatches enqueue tasks onto the main Tokio runtime.
The earlier `isolated` experiment placed enqueue work on the timer runtime too.
The new binary also records actual enqueue-start lateness. The paced profile rejects any
`late_dispatches` over the same 100 ms threshold, preserving the original shared-profile gate. This prevents the separated
timer from hiding delayed execution. No workload or deadline is relaxed.
NATS trial `0cb4ea0279` passed with 54,000 verified jobs, elapsed 60.061 seconds,
enqueue p95 63.54 ms, no late offers or dispatches, and maximum dispatch lateness
76.68 ms. The distinct 18-case `headroom-qualification-paced-pg16` sequence is
being evaluated; the trial alone is not qualification.


The paced sequence failed on Redis `b54dcee210` despite no late offers: 31
actual enqueue dispatches exceeded 100 ms. The new check prevents that result
from being treated as a pass. Reducing Tokio worker threads to two also failed
a standalone trial. A separate three-case heartbeat diagnostic failed all
three cases; an independent 10 ms heartbeat recorded a 125.17 ms scheduling
delay during the first case. This demonstrates delays outside the queue tasks,
not a proven exclusive cause or a reason to waive the timing requirement.

One of those Redis fixtures completed only 37,889 jobs by its original deadline.
Its server log recorded six completed AOF rewrites and the start of a normal
RDB snapshot. A separate `DOGRS_REDIS_REWRITE_MIN_MIB=1024` experiment raised the
minimum rewrite size from 64 MiB to 1 GiB, retaining the 100% growth trigger,
normal snapshots, always-fsync and fsync during rewrites. Trial `9333c88c5b`
completed two rewrites, started a snapshot, and passed with 54,000 verified jobs
in 60.075 seconds, enqueue p95 34.11 ms and maximum dispatch lateness 41.91 ms.
The [Redis persistence guidance](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)
explains that rewrite frequency can be tuned independently of the fsync policy.
The verifier checks the actual enabled rewrite settings for this profile.

An opt-in capacity-only `DOGRS_CAPACITY_FAIR_SCHEDULER=1` asks Tokio to check its
global task queue each scheduling tick. This evaluates dispatch fairness for
cross-runtime enqueue tasks; production runtime defaults remain unchanged.
NATS trial `c9f31fd4e4` used two Tokio threads, two Go execution threads per
server, the 1536 MiB Go budgets and balanced leaders. It passed with 54,000
verified jobs in 60.048 seconds, enqueue p95 264.80 ms and maximum dispatch
lateness 29.375 ms. The separate `headroom-qualification-fair-bounded` sequence
uses these fixed settings plus PostgreSQL enqueue concurrency 16 and the Redis
1 GiB rewrite minimum. Its verdict is failed/incomplete: PostgreSQL and Redis passed once and NATS passed once, but the second NATS case failed. No identical failed case was retried to replace that evidence.


### Final verdict for this experiment set

The last NATS fixture `21e2a10e41` accepted, completed and terminal-verified all
54,000 jobs, with no overload or operation errors. It nevertheless failed:
75 enqueue dispatches exceeded 100 ms (maximum 160.616 ms), and completion took
62.462 seconds, above the 62-second headroom limit. Enqueue p95 was 698.90 ms.
The sequence stopped after four of eighteen cases. The independent verifier
rejected the timing failure and the incomplete backend counts.

**No new Mac headroom qualification is claimed.** These are opt-in experiments
and evidence improvements, not adopted production defaults. The remaining
intermittent dispatch and drain delays are unresolved. The independent heartbeat
and Redis maintenance observations do not establish a single exclusive cause.
The user's IDE and build processes were not paused or stopped.

All 67 retained fixtures, including incomplete setup attempts and failures, are
indexed in [the experiment evidence](evidence/mac-headroom-experiments-2026-10-25.json).
Raw logs are retained in the parent task workspace at
`artifacts/dogrs-mac-capacity-2026-10-25/headroom-final-retained.tar`, with the
archive checksum in the index. The release binary built successfully and all
30 capacity workflow/verifier tests passed. The library test target contains
zero tests, so it is not claimed as additional runtime test coverage.

The capacity VM was stopped and returned to its original 16 GiB configuration.
Docker Desktop was restarted after the experiments; the temporary sleep
inhibitors were removed. No IDE or user build process was stopped.


### Scheduler and commit-path follow-up (2026-10-26 UTC)

The IDE and the user's macOS build remain running. The eight-CPU/eight-GiB
VM was restarted, with Docker Desktop temporarily stopped as authorized.
A system-wide scheduler trace in `headroom-scheduler-diagnostic/bf5149a109`
(the fixture directory includes the `dogrs-fault-` prefix) found a maximum
runnable delay of 12.84 ms for Tokio workers and 47.87 ms for NATS workers.
This instrumented fixture completed all jobs in 61.774 s, with no late offers
or dispatches, but enqueue p95 was 1753.90 ms. The trace adds overhead and is
not qualification evidence or proof that host contention never matters.

A lower-overhead CPU/stage diagnostic, `7dc6f067bd`, completed in 61.628 s
with 1013.64 ms enqueue p95. It made 35,688 enqueue batches for 54,000 jobs,
about 1.51 jobs per batch. Mean enqueue batch queue time was 78.63 ms and
mean batch execution time 36.49 ms. Mean time after the first staging response
was 29.30 ms; this includes client/network scheduling and durable provider work,
not an isolated fsync measurement. Candidate selection averaged 0.0081 ms.
All jobs were verified, with no operation errors or late dispatches.

A candidate changes the bounded enqueue collection window from 1 ms to 8 ms,
matching the existing metadata window. Full batches still dispatch immediately;
size bounds, independent metadata lanes, conditional revisions, uncertain-write
handling and final durable acknowledgements remain unchanged. Earlier window
experiments remain failed evidence; this comparison uses the newer paced/fair
runtime and bounded NATS memory profile rather than reclassifying them.

Two fresh candidate trials passed the stricter timing limits:

| Fixture | Completion | Enqueue p95 | Late offers / dispatches |
|---|---:|---:|---:|
| 310c59666d | 60.896 s | 620.62 ms | 0 / 0 |
| 7c723de929 | 60.130 s | 236.08 ms | 0 / 0 |

Both accepted, completed and verified all 54,000 jobs with no errors or overload.
Five writer unit tests and five live NATS writer tests passed, including atomic
visibility, delayed reply correlation, conflict splitting, deduplication and
metadata progress under saturated admission. The 30 capacity workflow/verifier
regression tests also passed. These two trials alone do not qualify headroom.

A separate fixed eighteen-case qualification uses binary SHA256
`f1a32ebe803414609eaeab8afe93bef8a625f10df5b2f46183f417ec91cc5316`,
under `headroom-qualification-batch8`. It checks the stricter per-case limits
immediately and stops on failure; no retries or replacement cases are allowed.
It stopped at the first PostgreSQL fixture, `55711f969e`: all 54,000 jobs
completed and verified in 60.071 s, enqueue p95 428.43 ms, no errors or overload,
but 200 enqueue dispatches exceeded 100 ms (maximum 210.983 ms). The offer clock
had no late offers (maximum 88.255 ms). This is a failed/incomplete sequence,
not a qualified batching change. The separate dispatch-worker diagnostic runs
four/two/two/four Tokio workers against fresh PostgreSQL fixtures to distinguish
the bounded runtime's dispatch backlog from provider throughput.


The PostgreSQL four/two/two/four-worker diagnostic completed without replacement
runs. Four-worker fixtures `ce14909bab` and `c338d17651` passed (60.143/60.056 s;
459.90/27.07 ms enqueue p95; no late offers/dispatches). Two-worker fixture
`f4cb1b8667` had 54 late dispatches, maximum 133.388 ms, despite completing all
jobs in 60.544 s with 85.79 ms p95; `9abc50b035` passed. This supports testing
more worker capacity, but does not prove the exclusive cause of every host stall.
A new eighteen-case sequence, `headroom-qualification-batch8-workers4`, uses four
Tokio workers for PostgreSQL/Redis and two for NATS. Other settings and limits
are unchanged. The prior failed sequence remains separate evidence.


The workers4 qualification stopped on its first NATS case, `547e6644b4`, after
PostgreSQL `fdd61095cf` and Redis `b2a2e29d7f` passed. NATS completed and verified
all jobs in 61.172 s with 190.33 ms enqueue p95, but recorded 104 late offers
(maximum 214.993 ms) and 161 late dispatches. The separate clock also stalled;
this cannot be described solely as a queue dispatch backlog.

A four-GiB VM with a 768 MiB Go budget per NATS server was rejected. Fixture
`99cffcf7d9` completed every job in 60.360 s, p95 92.73 ms and no late offers or
dispatches, but only 35,640 terminal snapshots were checked before the unchanged
30-second verification deadline. All three servers remained healthy. Increasing
snapshot read concurrency from 16 to 32 did not fix this: `df5c252386` verified
only 35,100 snapshots and also had 34 late offers/171 late dispatches. That read
concurrency experiment was reverted. The added test of large ordered snapshot
batches, repeated IDs and foreign-tenant rejection is retained.

Independent ten-millisecond heartbeat probes on macOS and Linux recorded the
same approximately 44.4 ms delay around UTC timestamp 1792975630.60 during the
first four-GiB experiment. This supports a shared scheduling effect, but does
not prove every observed stall has the same cause. Their later samples overlap
compilation and are not capacity evidence. A six-GiB/1024-MiB-per-server diagnostic
with four Tokio workers is under evaluation; no memory profile is qualified yet.


The six-GiB/eight-ms-batch diagnostic `12b85e85e8` completed all checks with no
late work, but elapsed 62.034 s exceeds the 62 s headroom limit. A sixteen-ms
enqueue collection window (metadata remains eight ms) reduced the timed workload
to 60.071 s and p95 35.53 ms in `1baa5cb642`, with zero late work; however the
sequential post-run readback verified only 52,380 jobs within its 30 s budget.

The readback implementation now overlaps four independent tenant checks after
freezing the timed capacity verdict. Each request retains the original batch
limit and one shared 30 s deadline. It now also checks returned count, exact
requested job IDs, tenant, Completed status and attempt=1, strengthening the
previous status/count-only verification. No drain deadline, workload, or offer
limit changes. Tests reject missing, duplicated, foreign and incorrectly completed
snapshots. The independent verifier rejects changed readback bounds or disabled
identity checks when the new evidence fields are present.

Diagnostic `4231905f97` with bounded parallel readback verified all 54,000 jobs:
60.056 s, enqueue p95 38.57 ms, maximum dispatch delay 24.969 ms, no late offers,
late dispatches, overload or errors. This remains one diagnostic, not qualification.
The new fixed sequence `headroom-qualification-6g-batch16` uses six GiB guest RAM,
four Tokio workers, two Go processors/server, 1024 MiB Go budget/server, the same
compact-four balanced streams and durability, PostgreSQL enqueue concurrency 16,
Redis rewrite minimum 1024 MiB, and the sixteen-ms enqueue window. All eighteen
fresh fixtures and their independent headroom checks are required.


## Final six-GiB Mac qualification — passed

The fixed sequence `postgres, redis, nats, nats, redis, postgres` ran three times
with fresh fixtures, no retries or replacement runs, and stopped-on-failure logic.
All 18 passed, covering 972,000 unique 64 KiB jobs. Each run admitted, completed
and verified all 54,000 jobs exactly once with zero errors, overload, late offers
or late dispatches. The independent verifier also accepted all 18.

| Backend | Fresh passes | Worst enqueue p95 | Slowest workload completion |
|---|---:|---:|---:|
| postgres | 6/6 | 19.19 ms | 60.059 s |
| redis | 6/6 | 37.47 ms | 60.068 s |
| nats | 6/6 | 436.29 ms | 60.204 s |

The slowest run left 4.796 seconds of the original five-second drain allowance,
and 1.796 seconds against the stricter 62-second limit. Worst enqueue p95 was
436.30 ms against the stricter 1,000 ms limit. This is measured latency and drain
headroom at 900/s, not a measurement of maximum sustainable throughput.

The qualified VM has eight vCPUs and six GiB RAM. Four Tokio workers, a separate
paced producer runtime, fair scheduling, balanced compact-four NATS streams,
two Go processors and a 1,024 MiB Go budget per NATS server were used. NATS keeps
three file-backed always-fsync replicas; Redis keeps always-fsync, automatic
rewrites and snapshots (rewrite floor 1,024 MiB); PostgreSQL keeps default durable
commits, external payload storage, pool limit 64 and enqueue concurrency 16.
NATS enqueue/update concurrency is eight/four per stream. Enqueue collection is
16 ms, metadata collection remains 8 ms, and full batches dispatch immediately.
Post-run readback overlaps four tenants under the unchanged shared 30-second
budget and checks exact identities, tenant, terminal status and attempt count.

The user's IDE and build were left running. Docker Desktop was temporarily
stopped as authorized. Qualification required 25 GiB free host disk space and
allowed up to 120 seconds between fixtures for APFS space reclamation; no files
were cleaned during the qualification. This is the configuration qualified here,
not a claim that every Mac memory profile or concurrent workload will pass.

The evidence JSON records the source hashes, executable SHA-256, all 18 reports,
and raw archive SHA-256. The complete raw archive is retained locally under
`artifacts/dogrs-mac-capacity-2026-10-25/headroom-qualification-6g-batch16.tar.gz`.
Verification uses `.github/scripts/verify_capacity_reports.py REPORT_ROOT
--nats-layout compact-4 --headroom --nats-leaders balanced
--producer-runtime paced --nats-memory-mib 1024 --fair-scheduler
--redis-rewrite-min-mib 1024`.

Validation: 31 capacity workflow/verifier Python tests, two snapshot-verification
Rust tests, ten NATS batch unit/integration tests, the release build and the
independent 18-fixture headroom verifier passed.

The measured candidate was based on `018939a` plus the capacity changes recorded
in the evidence hashes. Before publication it was rebased onto `41f5ec1` to
preserve concurrent calendar/tracing changes. Those commits did not change the
five hashed capacity sources; the 18-run result belongs to the measured candidate,
not a new qualification of every later workspace change.
