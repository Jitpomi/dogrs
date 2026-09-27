# Feature branch review — 27 September 2026

This audit compares the local and remote branch tips with `main` at `c467d57`
and the reviewed queue implementation at `bf38746`. Branches are retained so
the experiments remain reproducible. Integration does not certify a production
release or erase a failed capacity measurement.

| Branch | Reviewed tip | Disposition and reason |
|---|---|---|
| `docs/quickstart-ecommerce` | `6405891` | Already an ancestor of main. |
| `expand-core-serde-formats` | `57928e1` | Already an ancestor of main. |
| `feature/pluggable-transports` | remote `6937006`, local `565da71` | Both tips already ancestors of main, including the local-only commits. |
| `codex/dogrs-release` | `6c1225d` | Already an ancestor of main. |
| `codex/dogrs-hosted-validation` | `6f33487` | Already an ancestor of main. |
| `codex/production-readiness` | `4a55978` | Already integrated through squash PR #11: its tree equals main at `c467d57`. |
| `codex/queue-scale-recovery` | `bf38746` plus this integration | Integrate through PR #12. Per-job storage, bounded batching, lease fencing, worker cancellation, tests and recovery diagnostics. |
| `codex/nats-fsync-attribution` | `56d9d89` plus review corrections | Integrate the controlled experiment. Restore the general capacity workflow, remove blanket error suppression, and put the experiment in a dedicated manual workflow with validated results. |
| `codex/nats-caller-window` | `55b9713` | Exclude. Enlarging the admission window moved backlog and increased latency; ARM/shard experiments did not qualify the target. |
| `codex/nats-immutable-direct-reads` | `55f72c6` | Exclude. Additional leader/direct-read protocol complexity without demonstrated capacity improvement. |
| `codex/nats-metadata-piggyback` | `00caeee` | Exclude. Matched off/on/on/off runs did not establish improvement. Its blanket diagnostic error suppression must not enter main. |
| `codex/nats-packed-payloads` | `e0b3f2e` | Exclude. New payload format, cache and reclamation behavior are not sufficiently qualified; migration/cleanup concerns remain. |
| `codex/nats-payload-isolation` | `c1c1d73` | Exclude. Separating payload and metadata streams did not establish an improvement. |
| `codex/nats-window-attribution` | `0373edd` | Exclude. Producer coalescing delays and window changes did not establish an improvement. |

## Validation and merge boundary

The queue source at `bf38746` passed [all ten correctness, integration, security
and recovery CI jobs](https://github.com/Jitpomi/dogrs/actions/runs/36332851103).
The [strict capacity run](https://github.com/Jitpomi/dogrs/actions/runs/36332850985)
passed PostgreSQL and Redis, but failed NATS. The final integration runs CI again;
the strict NATS gate remains enabled and is not converted to an allowed failure.

The [profiled persistence comparison](https://github.com/Jitpomi/dogrs/actions/runs/36333925920)
and [unprofiled confirmation](https://github.com/Jitpomi/dogrs/actions/runs/36334661808)
isolate synchronous persistence as the source of measured backlog in this
deployment. They do not establish a universal NATS throughput limit or prove
that no further DogRS optimization is possible. Buffered cases weaken crash
durability and cannot qualify the production target.

The imported diagnostic now checks the workload, counters, terminal verification,
reported verdict and process exit status. An error-free, fully accounted capacity
miss is a completed experiment with an explicit failed workload gate. Setup,
runtime, integrity and incomplete-result failures fail the diagnostic workflow.
Regression tests cover both outcomes and run in ordinary CI. The general capacity
workflow continues to fail when its required workload does not pass.

See [production readiness](production-readiness.md) for the current limits.
No release, tag or package publication is included in this integration.
