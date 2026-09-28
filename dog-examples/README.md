# DogRS applications

These applications use the current workspace crates through local Cargo paths.
They follow the [application structure](../docs/application-structure.md). Each
example teaches a particular integration; none is a complete production product.

| Application | Demonstrates | Deliberate boundary |
| --- | --- | --- |
| `blog` | Poem, HTTP, CRUD, tenant-separated memory data, schemas, resolvers and hooks | Public local demo; a tenant header is not authentication |
| `auth-demo` | Actix, password/JWT/Google OAuth, durable local token revocation and atomic refresh consumption | Users and pending OAuth logins remain in memory; shared deployments need durable shared stores |
| `social-typedb` | Axum, TypeDB services, explicit remote TLS and schema initialization | Local raw-query playground; application tenant filtering is not automatic |
| `fleet-queue` | Axum, TypeDB, jobs, shutdown, optional persistent PostgreSQL queue, SSE | Public local fleet simulator; local PostgreSQL connector, not a hosted deployment recipe |
| `iot-devices` | HTTP/WebSocket service calls, hooks, public device broadcasts | Public loopback simulator; private channels need authorization |
| `music-blobs` | Streamed uploads/downloads, S3-compatible native multipart, resource limits, file recovery journal and durable receipts | Public loopback single-tenant media demo; resumable chunk sessions are process-local |
| `transport-demo` | One echo service over HTTP, gRPC, CLI and Iroh | Public stateless echo; Iroh is network reachable, so never send private data |
| `hosted-system` | Durable queue backends, idempotent effects, recovery and capacity validation | Synthetic infrastructure test harness, not real billing or payments |

Run commands and configuration live in each application's README. Use existing
local services or free resources; examples do not provision cloud infrastructure.

## What to copy

Keep services independent of the web framework. Compose chosen transports in
`app.rs` and mount them in the process entry point. Use hooks and schema modules
for their service concerns. Keep provider-specific connections in adapters.

Authentication must establish allowed tenants. Never treat a caller-supplied
`x-tenant-id` as proof of authorization. Memory examples deliberately demonstrate
routing and behavior, not durable storage. Retry side effects only with an
application idempotency/reconciliation strategy.

The module READMEs remain the reference for the full API, feature flags and limits:
[transport](../dog-transport/README.md), [authentication](../dog-auth/README.md),
[schemas](../dog-schema/README.md), [TypeDB](../dog-typedb/README.md),
[queues](../dog-queue/README.md), and [blobs](../dog-blob/README.md).

## Verify

```sh
python3 dog-examples/check_structure.py
cargo test --workspace --all-targets --locked
cargo check -p fleet-queue --features postgres --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Provider and feature-specific acceptance runs are separate. A compiling example
alone does not establish provider durability, throughput or production readiness.
