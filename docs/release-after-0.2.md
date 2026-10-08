# Next release after 0.2.0

Reviewed October 8, 2026, against published source commit `46f17ed` and merged
source `f069c0e`. This is a release proposal, not a publication announcement.
Manifests still declare 0.2.0. No versions have been reserved or uploaded.

## Proposed versions

| Package | Proposed version | Reason |
|---|---|---|
| dog-core | 0.2.1 | Additive listener failure counters and corrected compiling documentation. |
| dog-auth-local | 0.2.1 | Fix configured secret protection on paginated response envelopes. |
| dog-queue | 0.3.0 | Background notification ownership tightens the public generic bound to `Notifications + 'static`. |
| Other nine library crates | Keep current versions | No new runtime fix requiring their republication was identified in this audit. |

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

1. Apply the proposed versions to the three package manifests and refresh Cargo.lock.
   Existing compatible internal 0.2.0 requirements can accept core 0.2.1; consumers
   using the new counter API should require at least core 0.2.1 explicitly.
2. Run correctness/recovery CI on that exact commit and verify package archives,
   including optional feature builds. Workspace path builds alone do not validate
   resolution against the registry. Update any example that consumes the registry
   queue version when it needs the new behavior.
3. Publish core first, then local auth; queue has no dependency on another DogRS
   crate and can be released independently. Confirm each registry checksum and
   source commit before announcing availability. Publishing requires a separate
   explicit release instruction.
4. Smoke-test fresh consumer projects against the registry releases, not local
   path overrides. Retain the previous lockfile and backend backups for rollout;
   do not infer rolling storage compatibility from a successful Rust compile.

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
