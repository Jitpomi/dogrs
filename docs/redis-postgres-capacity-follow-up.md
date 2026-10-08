# Redis and PostgreSQL capacity follow-up

Final code: `be8b824`. Redis atomic completion and PostgreSQL per-connection
statement reuse are retained. **Admission batching was rejected and removed**
after the matched comparison below. Its options and modules described in the
historical experiment sections are not part of the final API.

Latest repeated capacity results: **Redis 4/4 and PostgreSQL 4/4 passed**.
Both also passed the final ordinary run, and final correctness/recovery CI passed.
The Redis repeated trials used `db404bf` (the retained Redis implementation is
unchanged in `be8b824`); PostgreSQL repeated trials and the ordinary run used
`be8b824`. PostgreSQL's identical independent-insert path has also failed on
other runners, so this is tested capacity, not proof of eliminated variability.
The final ordinary NATS capacity test failed; overall capacity CI is not green.

Target unchanged: 100 tenants × 9 jobs/s for 60 seconds, 64 KiB unique payloads,
54,000 offers and a five-second drain. No durability, admission or worker limits
were weakened. Each repeated comparison uses four fresh fixtures on one runner,
with diagnostics off/on/on/off. Separate workflow runs use different runners;
changes in their timings alone do not establish causation. "Overload" here is
the load generator declining an offer when its 32 in-flight requests per tenant
are occupied; those offers never reach the backend. This still fails the agreed
capacity gate, but is distinct from a database error or loss of an accepted job.

## Redis completion (`db404bf`)

The adapter previously read completion metadata and then sent a fenced write.
It now validates ownership and lease expiry and records completion in one Lua
script. The immutable payload stays untouched. Server time is authoritative;
result size checks, terminal state checks and active dedupe release are preserved.
The script explicitly preserves the empty JSON byte array when re-encoding
metadata, and computes all encoded values before starting mutations.

This uses Redis's [atomic script execution](https://redis.io/docs/latest/develop/programmability/eval-intro/)
without changing AOF always-fsync. It removes one client/server round trip per
completion, not the durability acknowledgment.

[Repeated comparison](https://github.com/Jitpomi/dogrs/actions/runs/37764226908):

| Trial | Diagnostics | Accepted | Completed/verified | Elapsed | Verdict |
|---|---|---:|---:|---:|---|
| 1 | Off | 54,000 | 54,000 | 60.075 s | Pass |
| 2 | On | 54,000 | 54,000 | 60.059 s | Pass |
| 3 | On | 54,000 | 54,000 | 60.058 s | Pass |
| 4 | Off | 54,000 | 54,000 | 60.057 s | Pass |

All four had zero errors and overload. The separate
[ordinary run](https://github.com/Jitpomi/dogrs/actions/runs/37764202743) also passed:
54,000 accepted/completed/verified, 60.057 s. This is a measured pass for these
runs, not a guarantee for all deployments or hardware.

## PostgreSQL: statement reuse was insufficient (`db404bf`)

Enqueue statements are cached per physical connection instead of parsed on each
admission. Reconnection creates a new statement cache. This alone did not resolve
the capacity failure: the ordinary run accepted/completed 49,490 and rejected
4,510 offers. The [repeated comparison](https://github.com/Jitpomi/dogrs/actions/runs/37764230490)
failed all four trials:

| Trial | Accepted | Completed by deadline | Overload | Operation errors |
|---|---:|---:|---:|---:|
| 1 | 39,142 | 32,930 | 13,704 | 1 |
| 2 | 48,897 | 46,166 | 5,103 | 0 |
| 3 | 49,245 | 44,434 | 4,755 | 0 |
| 4 | 51,496 | 51,496 | 2,504 | 0 |

Trial 1 reached the submission deadline with some commit outcomes unknown.
Those offers must not be counted as successful admission.

## Rejected experiment: PostgreSQL admission batches (`a29bb1f`)

In this rejected experiment, concurrent admissions shared an INSERT of at most 16 jobs by default.
Producer slots still bound pending jobs; a separate concurrency bound limits
executing batches. Duplicate active idempotency keys are split across statements
because PostgreSQL cannot update one conflict target twice in one INSERT.
Responses are sent only after the statement completes. Only explicitly aborted
deadlock/serialization outcomes can be retried; transport errors/timeouts retain
unknown-commit semantics. Canceled requests still queued are removed before SQL.
Each batch shares its transaction's rollback outcome. Setting
`PostgresOptions::enqueue_batch_size = 1` retains independent commits.

The [four-trial comparison](https://github.com/Jitpomi/dogrs/actions/runs/37765498292)
failed all four trials:

| Trial | Accepted | Completed by deadline | Overload | Operation errors |
|---|---:|---:|---:|---:|
| 1 | 51,219 | 51,219 | 2,781 | 0 |
| 2 | 51,418 | 51,418 | 2,582 | 0 |
| 3 | 51,944 | 51,831 | 1,900 | 1 |
| 4 | 51,727 | 51,727 | 2,273 | 0 |

The instrumented trials kept four admission statements busy almost continuously
(247–248 aggregate seconds of INSERT wait in 65 elapsed seconds). Connection pool
wait was negligible; producer-slot wait averaged about 1.62 seconds. The ordinary
run made 6,004 admission statements for roughly 52,000 jobs, confirming batching
was active, but it also failed: 51,755 accepted, 51,692 completed, 2,020 overload
and one submission-deadline error. None of these are capacity passes.

## Rejected experiment: dispatcher concurrency (`55b20bd`)

In this rejected experiment, the default admission dispatcher could execute up to eight batches (still one
quarter of the pool, capped at eight). Worker dispatchers retain their cap of four.
The existing producer-job cap and pool size are unchanged. Small default pools
still use one batch at a time, and explicit `batch_concurrency` overrides remain
honored. This removes the observed four-statement ceiling without increasing the
pool or pending-job budgets.

The [four-trial comparison](https://github.com/Jitpomi/dogrs/actions/runs/37766587965)
failed every trial, accepting 44,295 / 45,306 / 44,852 / 42,628 and completing
44,251 / 45,249 / 44,816 / 42,600. Each reached the submission deadline with
unfinished outcomes. Raising concurrency did not establish an improvement.

## Matched independent/batched comparison and removal

[Run `2e6194e`](https://github.com/Jitpomi/dogrs/actions/runs/37767662694) alternated
batch sizes 1 / 16 / 16 / 1 on one runner, with one binary, the same unique 64 KiB
payloads, rate, durable settings and diagnostic mode. Independent insert trials
both accepted/completed/verified 54,000, with zero errors/overload, in 60.071 and
60.081 seconds. Batched trials accepted/completed only 48,712 and 49,444, rejecting
5,288 and 4,556 respectively. All accepted jobs were verified.

That controlled comparison rejects admission batching as a performance fix for
this workload. `be8b824` removes the dispatcher, SQL, options and associated
experimental workflow controls. The established independent-insert path remains,
including the per-connection prepared statement cache. No threshold was lowered.

## Final PostgreSQL repeat (`be8b824`)

[Four fresh fixtures](https://github.com/Jitpomi/dogrs/actions/runs/37769318825)
all passed, in diagnostic off/on/on/off order:

| Trial | Accepted | Completed and verified | Elapsed |
|---|---:|---:|---:|
| 1 | 54,000 | 54,000 | 60.729 s |
| 2 | 54,000 | 54,000 | 60.766 s |
| 3 | 54,000 | 54,000 | 60.719 s |
| 4 | 54,000 | 54,000 | 60.784 s |

All four recorded zero errors, overload and late offers. The separate
[ordinary final run](https://github.com/Jitpomi/dogrs/actions/runs/37769311744)
passed PostgreSQL and Redis with 54,000 accepted/completed/verified each, in
60.080 s and 60.078 s respectively. Full correctness/recovery CI on `be8b824`
[passed](https://github.com/Jitpomi/dogrs/actions/runs/37769311683).

The final PostgreSQL adapter implementation matches its independent-insert path
on `db404bf`, whose earlier repeated run failed on another runner. Therefore the
latest four passes demonstrate capacity on the tested runner, not that statement
caching eliminated all variability. The controlled comparison supports rejecting
batching; it does not establish statement caching as the cause of later passes.


## Attribution harness correction

Native/DogRS admission diagnostics previously hard-coded 10 jobs/s/tenant while
full queue capacity defaulted to 9. The CLI rate, scheduler, reported count and
comparison validator now agree. Tests run the controller at 5, 9 and 10, reject
mismatched reports, and preserve failing stages without skipping later modes.
The old validator fails these regressions; all 12 Python controller/scope tests
pass after the correction. One-second live native and DogRS admission smokes
both offered/accepted/verified 900 jobs at the selected rate of 9.

A native diagnostic run was canceled during compilation before measurement when
its SQL path was found to mismatch the batched adapter. The first corrected
[native run](https://github.com/Jitpomi/dogrs/actions/runs/37768361378) verified all
54,000 payload-only offers, then stopped on the old controller count validator;
this is a harness failure and not a completed comparison. The final controller
fix is included in `be8b824`. Previous full-queue capacity measurements already
used 900/s and are unaffected.


## Final native comparison (`be8b824`)

[The corrected comparison](https://github.com/Jitpomi/dogrs/actions/runs/37769322723)
passed all eight measurements on one runner, reversing mode order for repeat 2.
Every measurement offered, accepted and verified 54,000 unique 64 KiB payloads,
with zero errors, overload or late offers.

| Mode | Repeat 1 elapsed | Repeat 2 elapsed | Scope |
|---|---:|---:|---|
| Native payload-only | 60.003 s | 60.002 s | Admission only |
| Native queue schema/INSERT | 60.004 s | 60.003 s | Admission only |
| DogRS admission | 60.002 s | 60.002 s | Admission only |
| Full DogRS queue | 60.075 s | 60.090 s | All 54,000 completed and verified |

The full-queue measurements are two additional capacity passes. The six
admission-only measurements do not test claims, completion or recovery. Native
layout shares the adapter's schema and single-row INSERT, so this comparison
cannot rule out SQL/layout costs. Native admission uses 64 clients while DogRS
reserves producer concurrency at 48; it is not a pure measurement of Rust
framework overhead. All modes passed on this runner, so the comparison does not
reproduce or explain the earlier failures. It does establish that the current
adapter can meet this target without admission batching or weaker durability.

## Correctness evidence

- Live Redis cross-connection contract and two new completion regressions passed:
  competing completions, wrong owner, result limit, escaped metadata/dedupe keys,
  empty/64 KiB payloads, result preservation and server-clock calendar boundaries.
- Final live PostgreSQL reconnect/initialization tests and 15 storage tests passed.
  The rejected batching experiment also passed its 16-test storage suite, including
  duplicate submissions and producer row locks while unrelated workers complete;
  those historical correctness passes did not justify retaining its slower path.
- Unit tests and strict Clippy passed. Full CI for `db404bf`
  [passed](https://github.com/Jitpomi/dogrs/actions/runs/37764202753). Full CI for
  `a29bb1f` also [passed](https://github.com/Jitpomi/dogrs/actions/runs/37765488076).
- Local SIGKILL/restart recovery passed for Redis and PostgreSQL on `db404bf`, and
  again for PostgreSQL on `a29bb1f`; these local outages were three seconds. This
  does not certify provider failover or backup restoration.

## NATS qualification

The unchanged NATS adapter failed the ordinary `db404bf` capacity run: 32,881
accepted, 22,590 completed, 20,438 overload and one submission-deadline error.
The earlier four passing trials remain valid historical results, but they do not
establish consistent capacity across runners. This failure is not classified as
infrastructure-only, and it must not be omitted from an overall readiness claim.

The ordinary `a29bb1f` run passed Redis (54,000 verified, 60.054 s) and NATS
(54,000 verified, 61.185 s). These later passes do not erase the preceding NATS
failure. [Run](https://github.com/Jitpomi/dogrs/actions/runs/37765487846).

Full CI for `55b20bd` [passed](https://github.com/Jitpomi/dogrs/actions/runs/37766584253)
and for `2e6194e` [passed](https://github.com/Jitpomi/dogrs/actions/runs/37767636710).
These correctness passes did not override their failed capacity gates.

The final ordinary NATS run failed again: 30,653 accepted, 20,513 completed by
deadline (20,625 verified later), 22,494 overload and one submission-deadline
error. No NATS adapter code changed in this work. The passing PostgreSQL/Redis
results do not make that overall workflow green.
