# DogRS

A modular Rust framework for services, hooks, tenant context and pluggable transports.
Write a service once and expose it through HTTP, gRPC, a CLI or Iroh.

This checkout prepares the **0.2.0 API**. It includes breaking changes from the
published 0.1.x crates; changing this repository does not publish new crates.io
versions. Read the [migration notes](docs/release-0.2.md) before updating an app.

## Start here

```sh
cargo run -p transport-demo -- http
# Or: cargo run -p transport-demo -- grpc
# Or: cargo run -p transport-demo -- cli
```

The [quickstart](docs/quickstart.md) includes requests for each mode and requires
no database or credentials. [Transport documentation](dog-transport/README.md)
explains server composition and Iroh's shared endpoint support.

## Workspace

| Crate | Purpose |
| --- | --- |
| `dog-core` | Services, hooks, application builder and explicit tenant context |
| `dog-transport` | HTTP/Tower, gRPC/Tonic, NDJSON CLI, Iroh and HTTP streaming adapters |
| `dog-axum` | Axum-specific REST helpers |
| `dog-auth` | Authentication strategies and typed JWT access/refresh verification |
| `dog-auth-local` | Password authentication and password protection hooks |
| `dog-auth-oauth` | Provider-verified OAuth identities and authorization-code support |
| `dog-typedb` | TypeDB queries, transactions and atomic schema loading |
| `dog-queue` | Typed jobs, persistent ledgers and broker integrations |
| `dog-blob` | Blob storage and media utilities |
| `dog-schema`, `dog-schema-macros`, `dog-schema-validator` | Schemas and runtime validation |

A tenant context routes an operation; it does not authenticate or authorize the
caller. Applications must install authentication and authorization hooks and derive
allowed tenants from verified identity. Configure public TLS, request concurrency
and deployment limits in the hosting server. See the release notes for the tested
scope and remaining application responsibilities.

## Queues

PostgreSQL, Redis and NATS JetStream are persistent job ledgers. RabbitMQ, both Kafka
clients, AWS SQS and Google Pub/Sub operate with a durable ledger and broker wakeup
notifications. Memory is available for tests and ephemeral work. See the
[queue guide](dog-queue/README.md) for the API, capacity limits and migration.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

CI also checks transport feature combinations, alternative JWT cryptography,
TypeDB 3.13.6, persistent queues and local broker/emulator integrations. Cloud IAM,
provider quotas and deployment-specific TLS configuration are not emulator-tested.

Examples live in `dog-examples/`. `auth-demo` has a browser-bound OAuth flow with
an in-memory demonstration session store; multi-instance deployments need shared
session storage. Example apps are not published as library crates.

Built by [JITPOMI](https://github.com/Jitpomi).
