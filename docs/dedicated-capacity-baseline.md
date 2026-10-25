# Dedicated capacity baseline

Capacity is an environment-specific qualification, not a guarantee for every
GitHub-hosted machine. Ordinary CI continues to run correctness and recovery
checks. `Provider capacity diagnostics` is now manual-only; historical failures
remain valid evidence and are not reclassified as passes.

`Dedicated capacity baseline` is also manual-only. It runs PostgreSQL, Redis,
NATS, NATS, Redis, PostgreSQL serially on the designated host, using fresh
fixtures. Every case must admit, complete and verify all 54,000 offers: 100
tenants at nine jobs/second each for 60 seconds, 64 KiB unique payloads and the
existing five-second drain. Durable commit settings remain unchanged. All
failures are retained, even if later cases pass.

## Host selection and operation

The initial candidate is a Lima Ubuntu 24.04 ARM64 VM named `dogrs-capacity`,
with four virtual CPUs, 16 GiB memory and a 40 GiB virtual disk on the existing
Mac. No paid service is needed. Host folders are not mounted into the VM.
This is a candidate, not yet a validated 900/s baseline. Mac activity, sleep,
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
branch. Use `mode=all-backends`, 65,536-byte payloads, 60 seconds and rate nine,
with all other settings at baseline defaults.

Record the commit, toolchain, runner name, OS, CPU, Docker version and each
fixture's hardware/environment artifacts. Changing the VM resources, toolchain,
container images or storage invalidates comparisons until requalified. Preserve
the artifacts of failures as well as passes. A repeatable host does not make a
failing workload pass automatically.

## Qualification status

Pending provisioning and repeated measurements. No dedicated-host capacity
claim or production qualification has been made. Hosted-runner diagnostics may
still be run manually to study portability; they must not be used to cherry-pick
an apparent baseline pass.
