# DogRS 0.2.0 migration and release notes

This is the consolidated transport API from `feature/pluggable-transports`,
including the local Iroh endpoint and blob-helper commits. The ecommerce docs
branch was merged and its obsolete examples replaced by a runnable transport
example. The serde branch was already contained in main; the retired builder
branch was already represented by an identical squash commit.

## Transports

`DogApp` dispatches the shared `DogRequest` envelope. Server adapters set transport
identity; callers cannot select `Internal` over HTTP, gRPC, CLI or Iroh to bypass
authentication hooks. HTTP request bodies and binary/line transports have bounded
input sizes. Internal errors are sanitized before returning to remote clients.

- HTTP: `IntoDogService<HttpOptions>` produces a Tower service.
- gRPC: `IntoDogService<GrpcOptions>` produces `DogGrpcService`. Its generated
  `dog.v1.DogTransport/Call` method accepts protobuf bytes containing DogRequest JSON.
  `into_server()` supports composition with Tonic TLS and interceptors;
  `serve_with_shutdown()` is a plaintext convenience for local use or a TLS proxy.
- CLI: `IntoDogService<CliOptions>` produces a newline-delimited JSON dispatcher,
  with `run(reader, writer)` and `run_stdio()` entry points.
- Iroh: `app.into_service(IrohOptions { ... }).await?` replaces the blocking setup
  call and reports startup failures. Existing endpoint and RouterBuilder composition
  remain available. Request streams are bounded across peers and owned by the
  connection; read/write framing has size and time limits.

Global WebSocket broadcasts are disabled by default. Set `ws.public_broadcasts`
to the string `"true"` only for intentionally public events. Private event routing
must authorize subscribers and filter tenants. SSE/WebSocket option structs do
not automatically apply every hosting-server setting; configure the server itself.

## Authentication

`AuthenticationService::install` now stores the service where lookup hooks expect
it. Deserialization cannot set `authenticated` or `auth_result`. Access verification
rejects refresh and identity tokens; use `verify_refresh_token` when appropriate.

HMAC requires a secret. RSA and ECDSA use configured private/public PEM paths;
`jwt-pem` is enabled by default. JWT uses the AWS-LC backend. The `jwt-rust-crypto` feature is removed because
its RSA dependency has the unresolved RUSTSEC-2023-0071 timing advisory. For builds
without PEM support, disable default features and select `jwt-aws-lc-rs` without
`jwt-pem`; the old `jwt-no-pem` flag alone does not disable an additive Cargo feature.
Keys are cached per authentication instance. Construct a new instance to rotate
keys. Protect key files through deployment configuration; test fixtures are
intentionally disposable public test keys.

Local password hashing runs in bounded blocking tasks, and nested password fields
are removed from successful authentication results.

OAuth rejects caller-supplied profiles and requires a configured provider to verify
a token and fetch an identity. Raw provider access tokens and authorization codes
are not returned in authentication results. Provider implementations must now accept
callback state in `exchange_code(code, state, ctx)`.

The built-in OAuth2 provider requires `OAuthCallbackVerifier<P>` at construction.
`authorize_url()` returns `OAuthAuthorization { url, state, code_verifier }`. Store
the state and verifier server-side, bind them to the initiating browser session,
expire them promptly and consume them atomically once. The verifier validates that
binding and returns the stored PKCE verifier before token exchange. The demo shows
a bounded single-process implementation with HttpOnly, SameSite cookies. Provider
HTTP clients reject redirects and use timeouts.

## TypeDB

The driver and live server tests use **3.13.6**. A 3.11.5 server is not compatible
with the resolved 3.13 driver protocol. Upgrade and test your server explicitly.

`read`, `write` and `schema` use their named transaction types; write cannot elevate
to a schema transaction. Schema files execute in one transaction and commit only if
all succeed. Missing files and schema errors are reported. Schema initialization is
not an automatic migration engine and does not ignore duplicate definitions.
Result limits bound returned rows while draining the query stream before commit.

## Queues

The old remote-broker implementations were process-local prototypes. The new
PostgreSQL, Redis and JetStream ledgers persist job state, lease ownership and
scheduled retries across workers. PostgreSQL reconnects after a closed connection;
ambiguous commits are not blindly retried. Applications should use idempotency keys
and inspect status when the outcome of a network failure is unknown.

RabbitMQ, Kafka, SQS and Pub/Sub constructors now require a persistent `JobLedger`
and caller-configured vendor clients. Their messages are wakeups; acknowledging
one cannot complete or remove the ledger job. Polling continues during broker
outages. This replaces the early acknowledgement and process-local retry timers
that could lose work. See [queue documentation](../dog-queue/README.md) for examples,
operator requirements, retention and capacity limits.

These are at-least-once queues. Handlers must make their side effects idempotent.
Redis and JetStream tenant-state stores target modest job volume. PostgreSQL v2
uses independent indexed rows, pooled connections and binary payloads; its upgrade
requires an explicit offline migration. Aggregate capacity remains workload-dependent. Terminal history needs retention; persisted result references are
bounded. The unused SQLite/SQLx dependency flags are removed; they never exposed a backend.
UI/workflow placeholders are not implemented products.

## Hosted system validation

The hosted acceptance application in `dog-examples/hosted-system` exercises real
TLS connections and independent API/worker processes. It exposed and fixes lease
heartbeat drift under backend latency, unnecessary writes during status reads,
and account-specific JetStream payload limits. This is additional evidence, not
a substitute for measuring production scale and provider failover.

## Verification scope

The repository tests cover service dispatch, authentication regressions, schema
validation, HTTP errors, real gRPC and CLI calls, Iroh lifecycle/blob helpers,
TypeDB transaction behavior, independent queue clients, broker notifications,
worker reconstruction and broker-outage recovery. CI runs the local services and
SQS/Pub/Sub emulators. It does not create paid cloud resources or validate a user's
cloud IAM, service quotas, backup restoration or cluster failure policy.

No new crate versions are published by merging this change. Update app dependencies
only after selecting the intended Git revision or publishing the versioned crates.

## Dependency security

The release removes legacy Hyper/rustls connectors from AWS SDK feature selection,
updates NATS to the maintained client/TLS stack, and disables the Actix demo's
unused legacy HTTP/2 implementation. Tonic and Axum retain current HTTP/2 support.
The optional RustCrypto JWT backend is removed rather than exposing the unpatched
RSA private-key timing path. Use `jwt-aws-lc-rs` for HMAC, RSA and ECDSA support.
