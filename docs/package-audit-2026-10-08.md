# Package audit and release inventory — October 8, 2026

Scope: the 12 library crates in the workspace, their documented contracts,
regression coverage and published-source alignment. Examples are applications,
not additional framework libraries. This is an evidence inventory and focused
correctness review, not proof that every deployment is production-ready.

## Published/source alignment

The crates.io sparse index reports 0.2.0 as the latest non-yanked version of all
12 crates below. Downloaded archives were checked against the index's SHA-256
checksums. Each archive's `.cargo_vcs_info.json` names commit `46f17ed`.
Compared archive `src/` files and README bytes to the checkout; Cargo manifests,
resolved dependencies and compiled artifacts were not claimed identical.
The crates.io API returned 403, so the canonical sparse index and static archives
were used instead. No TLS verification was disabled.

| Crate | Review/test focus | Source/release finding |
|---|---|---|
| dog-core | Event delivery, hooks, runtime independence, README API | Adds failure counter and six event tests; replaces invalid README examples. Not yet released. |
| dog-transport | WebSocket/SSE lifecycle, tenant/header boundaries, transport deadlines | Source matched archive at inventory; 19 realtime tests pass. No sustained realtime capacity claim. |
| dog-auth | JWT claim checks, refresh replay, revocation, credential error handling | Source matched archive; auth regressions pass. Supplied stores and tenant authorization remain application contracts. |
| dog-auth-local | Password limits, hash redaction, response protection | Reproduced and fixed protection bypass on a pagination envelope. Not yet released. |
| dog-auth-oauth | One-use callback state, provider HTTP bounds, resolver requirements | Source matched archive; client-enabled tests pass, including loopback exchange. No live external identity-provider certification. |
| dog-typedb | Explicit transaction routing, result bounds, schema atomicity | Source matched archive; local tests pass. Live local-server transaction test is separately run by CI; historical Cloud evidence is not a new Cloud test. |
| dog-blob | Resource admission, staging cleanup, multipart ownership/recovery | Runtime source matches archive apart from a deprecation-warning annotation in resources.rs. Local hardening/recovery tests pass; S3-compatible live tests are a separate CI job. |
| dog-schema | Method gating, validation errors, generated schema behavior | Source matched archive; validation tests and doctests pass. |
| dog-schema-macros | Macro input rejection and generated API | Source matched archive; tests plus dog-schema generated-code cases pass. |
| dog-schema-validator | Nested validation paths and sanitized decode errors | Source matched archive; unit tests and doctests pass. |
| dog-axum | Deprecated wrapper delegation, multipart and compatibility | Source matched archive; compatibility/regression tests pass. New applications should use dog-transport. |
| dog-queue | Previously reviewed adapters and recovery | PR #24 merged; many adapter fixes postdate 0.2.0 and are not in the published crate. Capacity limits remain recorded separately. |

This table includes the findings made during this pass. A matching archive does
not certify the source, dependencies or consumer configuration. Publishing an
updated patch release is a separate action; no versions or releases changed here.

## Fixes and explicit decisions

- `ProtectHook` previously stripped page rows but returned the surrounding object
  without applying its own configured protection rules. A regression reproduced
  retained envelope `password` and nested `token` fields. The fix applies the
  rules to the envelope as well, preserving pagination and safe row fields.
- `DogApp::event_listener_failures()` and `DogEventHub::listener_failures()` expose
  returned listener errors without retaining sensitive data. Application dispatch
  still continues after listener failure; it does not report an already committed
  service mutation as failed. Direct hub emission retains first-error return.
- Listener execution remains sequential in the request future. Cancellation and
  once-selection semantics are now tested and documented. A runtime-specific
  timeout or unlimited detached delivery was not added to the runtime-independent
  core. Application-owned listeners must use bounded handoff for expensive work.
- The dog-core README used nonexistent associated types, tenant builder methods
  and service signatures, and implied automatic tenant isolation. It now reflects
  the actual API and is included as a compiling crate doctest.
- The production-readiness document incorrectly presented 1,000/s as the current
  default. Its current section now says 900/s and distinguishes historical tests.

## Validation

Local commands on this branch:

- `cargo test -p dog-core -p dog-auth-local --locked`: all pass, including six new
  event tests and the new response-protection regression; core README compiles.
- Core tests also pass with no default features and with only `serde` enabled.
- `cargo test -p dog-auth -p dog-auth-oauth -p dog-schema -p dog-schema-macros -p dog-schema-validator -p dog-typedb -p dog-blob -p dog-axum --features dog-auth-oauth/oauth2-client --all-targets --locked`:
  114 passed, one live TypeDB test explicitly ignored locally.
- Matching package doctests pass; ignored examples are not counted as verified.
- `cargo test -p dog-transport --features http --test realtime --locked`: 19 passed
  on the pre-fix baseline; rerun after core changes is recorded in the PR.
- Clippy for changed code (`dog-core`, `dog-auth-local`, all targets, warnings
  denied), formatting and diff checks pass.

The full PR CI must separately validate optional features, real local databases,
brokers, S3-compatible storage and recovery. Earlier green CI at `02f45d8` is not
substituted for the changed tree. No new hosted cloud credentials or paid
resources were required.

## Remaining operational/release limits

Maintain the 900/s queue default and visible failed capacity runs. The optional
1,000/s stress experiment is not a shared passing target. There is no agreed or
validated sustained realtime workload yet. Neither local tests nor provider
emulators certify every managed service's failover, backup restoration or
application authorization. A release needs an explicit version/dependency update
and packaging verification so consumers actually receive these new fixes.
