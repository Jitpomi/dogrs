# Dedicated capacity baseline

Capacity is an environment-specific qualification, not a guarantee for every
GitHub-hosted machine. Ordinary CI continues to run correctness and recovery
checks. `Provider capacity diagnostics` is now manual-only; historical failures
remain valid evidence and are not reclassified as passes.

`Dedicated capacity baseline` is also manual-only. It runs PostgreSQL, Redis,
NATS, NATS, Redis, PostgreSQL serially, repeated for three full rounds (18
measurements) on the designated host, using fresh
fixtures. Every case must admit, complete and verify all 54,000 offers: 100
tenants at nine jobs/second each for 60 seconds, 64 KiB unique payloads and the
existing five-second drain. Durable commit settings remain unchanged. All
failures are retained, even if later cases pass.

## Host selection and operation

The initial candidate is a Lima Ubuntu 24.04 ARM64 VM named `dogrs-capacity`,
with eight virtual CPUs, 16 GiB memory and a 40 GiB virtual disk on the existing
Mac. No paid service is needed. Host folders are not mounted into the VM.
The `compact-4` profile passed the complete Mac qualification described below. Mac activity, sleep,
power and storage contention can still affect it. Run on AC power with other
heavy workloads stopped. Do not claim parity with the previous AMD runners.

Register only this VM with runner labels `self-hosted`, `linux`, and
`dogrs-capacity`. Set repository variable `DOGRS_CAPACITY_RUNNER_NAME` to its
exact runner name. Both dedicated workflows share a concurrency group and
refuse an unexpected host. Use an ephemeral runner started deliberately for a
reviewed commit; do not install a continuously listening runner on the Mac.
Never add pull-request or pull_request_target triggers to the self-hosted path.
Do not provide production secrets or host SSH agents to the VM.

The manual diagnostics workflow also accepts `runner=dogrs-capacity` so the
candidate can be tested before the new baseline workflow reaches the default
branch. Use `mode=qualification`, 65,536-byte payloads, 60 seconds and rate nine,
with all other settings at baseline defaults. Qualification rejects reduced
rate/duration and profiling. An isolated `ubuntu-latest` x64 job may also run
the eighteen-case qualification with the pinned toolchain; its result applies
only to the hardware recorded in that run, not every hosted runner. A failure anywhere fails the entire
qualification; it is not retried automatically or replaced by a later pass.
An independent report verifier also requires six complete results per backend,
checks the workload and deadline fields, and validates recorded PostgreSQL,
Redis and NATS durability settings. Missing evidence fails qualification.

Record the commit, toolchain, runner name, OS, CPU, Docker version and each
fixture's hardware/environment artifacts. Changing the VM resources, toolchain,
container images or storage invalidates comparisons until requalified. Preserve
the artifacts of failures as well as passes. A repeatable host does not make a
failing workload pass automatically.

## Qualification status

The VM and ephemeral runner `dogrs-capacity-macvm` have been provisioned.
Rust is pinned to 1.99.1, matching the earlier passing hosted build. Docker
reports eight CPUs and approximately 16 GiB of guest memory. No host home directory
is mounted. Initial validation is
[run 40217023372](https://github.com/Jitpomi/dogrs/actions/runs/40217023372).
The initial four-CPU run failed: Redis passed twice, PostgreSQL completed every
job but recorded 14 and 9 late offers, and NATS failed both cases. High guest CPU
pressure was observed during NATS. The VM was then resized to eight CPUs.
The repeated eight-CPU qualification is
[run 40217853256](https://github.com/Jitpomi/dogrs/actions/runs/40217853256).
That original-layout attempt did not establish a dedicated-host capacity claim.
The later compact-layout qualification is documented below. Hosted diagnostics may
still be run manually to study portability; they must not be used to cherry-pick
an apparent baseline pass.


### Eight-CPU results

The six-case run completed with five passes. PostgreSQL and Redis each passed
both repetitions, accepting, completing and verifying all 54,000 jobs with no
late offers, overload or operation errors. NATS passed one repetition; the other
accepted/completed 53,896 jobs and rejected 104 offers, with no late offers or
operation errors. That run remains failed. A fixed VM improves control of the
experiment but does not eliminate capacity variability or establish a reliable
three-provider 900/s guarantee.

The ephemeral runner unregisters after each job. The VM is stopped between
benchmark sessions to release the Mac's resources. To run again, start it with
`limactl start --tty=false dogrs-capacity`, register a new ephemeral runner in
that VM with the same name and labels using a fresh GitHub registration token,
and dispatch the reviewed workflow ref. Start `~/actions-runner/run.sh` inside
the guest with `~/.cargo/env` sourced. Do not print or commit registration tokens.
Keep the Mac awake and otherwise idle throughout the measurement, then stop the
VM after artifact upload. No login-time service has been installed.

### Hosted x64 qualification

The predeclared eighteen-case qualification passed in
[run 40224103288](https://github.com/Jitpomi/dogrs/actions/runs/40224103288):
six passes per backend at 900/s, 972,000 jobs total, with zero late offers,
overload or operation errors. The independently verified result applies to its
four-logical-CPU AMD EPYC 7763 runner, Rust 1.99.1 and recorded storage setup.
It does not qualify the Mac VM or all `ubuntu-latest` allocations. See
[the investigation report](capacity-reliability-2026-10-25.md#completed-x64-qualification)
for timing, storage-durability limitations and the retained failed experiments.

### Explicit layout trials

The dedicated workflow defaults to `nats_layout=compact-4`, the tested Mac profile.
`original-16` remains available for explicit comparisons using fresh fixtures. The compact layout uses four buckets with
eight enqueue and four metadata lanes per bucket; the original uses sixteen
buckets with two enqueue and one metadata lane each. Both total 32 enqueue and
16 metadata execution lanes. This does not change replication, fsync, rate,
payload size, worker count or drain deadline. The verifier must receive the same
explicit layout using `--nats-layout compact-4`. It checks the recorded lanes and
stream count, and will not infer a profile from the result. The Mac compact-layout
qualification passed all eighteen measurements at 900/s. This is workload capacity
evidence, not a general production certification. One NATS run took 64.936 seconds
against the 65-second limit, so do not infer spare capacity. Full results and
operating conditions are in [the investigation](capacity-reliability-2026-10-25.md#verified-mac-vm-qualification).
Never apply a changed shard count to existing stored jobs without migration.
