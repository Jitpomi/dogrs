# Release 0.4.0

All twelve library crates are prepared as **0.4.0**, with internal dependencies
updated together. This coordinated release provides applications with a unified,
consistent framework version across the ecosystem.

## Key Changes in 0.4.0

### `dog-queue`
- **Distributed Tracing**: Persist W3C `traceparent` metadata on `JobMessage` to provide end-to-end tracing across producer enqueues and worker execution attempts/retries without raw baggage or secret leakage.
- **OpenTelemetry Restoration**: Restored `tracing-opentelemetry` feature and dynamic compatibility for `JobSpan` trait, eliminating false-alarm interrupted spans on terminal states (`completed`, `failed`, `canceled`).
- **Worker Concurrency**: Clean lease release and reaper race handling during worker acknowledgment (`ack_fail` and `ack_complete`).

### `dog-transport`
- **Native Real-Time Streaming**: Direct support for bidirectional WebSocket and Server-Sent Events (SSE) connections.
- **`DogEventHub` Bridge**: Snapshot-based hub bridge with lease-fenced progress reporting (`Memory`, `Redis`, `PostgreSQL`, `JetStream`) and authorized opt-in completed result streaming (bounded to 4 KiB).
- **Tenant Isolation**: Broadcast envelopes carry tenant identifiers to enforce workspace boundary rules.

### `dog-blob`
- **File-Journal Lease Release**: File leases and blocking mutations now share an `Arc`-owned lock guard ensuring operating system file locks are reliably unlocked when the last journal operation finishes.
- **Upload Hardening**: Bounded multi-part chunk staging, strict tenant scoping, and isolated upload manifests.

### Dependency & Supply Chain Security
- **`cargo-audit` Zero CVEs**: Purged vulnerable transitive `zstd`, `zstd-safe`, and `zstd-sys` dependencies by disabling unused compression defaults on `rskafka`.

## Publication Order

When publishing to crates.io, publish in strict dependency order:
1. `dog-core`
2. `dog-schema-macros`
3. `dog-schema` (depends on `dog-core`, `dog-schema-macros`)
4. `dog-schema-validator` (depends on `dog-schema`, `dog-core`)
5. `dog-auth` (depends on `dog-core`)
6. `dog-auth-local` (depends on `dog-auth`, `dog-core`)
7. `dog-auth-oauth` (depends on `dog-auth`, `dog-core`)
8. `dog-queue` (independent)
9. `dog-blob` (independent)
10. `dog-typedb` (depends on `dog-core`)
11. `dog-transport` (depends on `dog-core`, `dog-queue`, `dog-auth`)
12. `dog-axum` (depends on `dog-transport`, `dog-core`, `dog-auth`)
