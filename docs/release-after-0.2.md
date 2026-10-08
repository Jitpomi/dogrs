# Next release after 0.2.0

All twelve library crates are prepared as **0.3.0**, with internal dependencies
updated together. This coordinated version lets applications adopt one framework
release. Cargo requires an explicit upgrade from 0.2; it does not mean every crate
has breaking source changes. Publication status must be verified in crates.io.

## Changes since 0.2.0

- `dog-core`: additive event listener failure counters and corrected compiling documentation.
- `dog-auth-local`: fix configured secret protection on paginated response envelopes.
- `dog-queue`: durable-write and recovery fixes; background notifications now require
  `Notifications + 'static`.
- Other nine libraries: coordinated dependency/version updates; `dog-axum` remains
  deprecated in favor of `dog-transport`.

Cargo treats tightening a generic bound as incompatible; before 1.0, changing the
second version component expresses that boundary. See the
[Cargo compatibility guidance](https://doc.rust-lang.org/cargo/reference/semver.html#major-tightening-generic-bounds).
This is a targeted compatibility review, not an exhaustive automated API diff.
The Redis dependency upgrade from 0.28.2 to 1.7.1 is internal to the reviewed
Redis backend constructors, which accept DogRS configuration rather than a Redis
client. The custom notification lifetime restriction alone warrants the queue bump.

## Consumer changes

For custom queue notifications, replace borrowed client/configuration fields with
owned values or `Arc` handles. `'static` does not mean leaking memory: the value
must contain no non-static borrowed references. Generic helpers constructing a
`BrokerBackend<N>` must also require `N: Notifications + 'static`.

Keep the Tokio runtime alive for the backend's active lifetime. Stop producers and
workers before calling `shutdown_notifications().await`. Dropping aborts wrapper
tasks but does not await adapter cleanup. Reuse after explicit shutdown restarts
notification tasks. Counters are asynchronous; a successful enqueue establishes a
ledger commit, not successful broker publication. Continue polling/recovery even
when notifications fail. See [the queue README](../dog-queue/README.md).

For local auth, pagination envelope fields now obey the same configured protection
rules as rows. Applications that used a protected field name for response metadata
must rename it; they should not disable secret protection to preserve that metadata.

For core, listener errors can be monitored using the new counter. Listener execution
still happens sequentially in the caller future. The counter does not measure
cancellation or panic, and is not a durable audit log.

## Release procedure

1. Verify package archives and correctness/recovery CI. Workspace path builds alone
   do not validate registry resolution. All internal library requirements are 0.3.0.
2. Publish in dependency order: core and schema macros before their dependents;
   auth before local/OAuth/transport; transport before axum; schema before validator.
   Queue and blob can publish independently.
3. Confirm registry checksums and source commits before announcing availability.
   Smoke-test registry packages without local path overrides. Retain the previous
   lockfile and backend backups for rollout; compilation does not prove rolling
   storage compatibility.

## Evidence and limits

[PR #25 correctness/recovery CI](https://github.com/Jitpomi/dogrs/actions/runs/37788570812)
passed all eleven jobs on `eecc450`, including live integration and recovery tests.
The subsequent merge commit is `f069c0e`; its CI result must be checked separately.
See the [package inventory](package-audit-2026-10-08.md) for per-package scope.

The default capacity gate remains 100 tenants, 900 jobs/s, 64 KiB payloads, a
60-second arrival window and five-second drain. A shared-host run passed all three
backends, but [the later matrix run](https://github.com/Jitpomi/dogrs/actions/runs/37788570890)
passed PostgreSQL and Redis and failed NATS. Do not market 900/s as a universal
throughput guarantee or describe the full capacity matrix as passing.

A subsequent release-candidate run on `708c9a9` also missed the gate for all three:
PostgreSQL accepted/completed 46,182/44,959; Redis 52,900/42,393; JetStream
32,109/21,831. PostgreSQL and Redis recorded zero operation errors; JetStream
recorded a submission deadline with unknown unfinished commit outcomes. These
results reinforce that the gate is deployment-dependent and not a release throughput
guarantee. [Release-candidate capacity run](https://github.com/Jitpomi/dogrs/actions/runs/37792769166).
