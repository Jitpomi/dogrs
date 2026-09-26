# DogRS quickstart

One service can run behind HTTP, gRPC, or a command-line session. Start with the checked-in, stateless echo example; it needs no database or credentials.

## Run the same service through three transports

From this repository:

```sh
cargo run -p transport-demo -- http
```

In another terminal:

```sh
curl --fail http://127.0.0.1:3000/echo -H 'Content-Type: application/json' -d '{"hello":"world"}'
```

For gRPC:

```sh
cargo run -p transport-demo -- grpc
```

The server implements `dog.v1.DogTransport/Call` and enables reflection in this demo. The request has one `bytes request_json` field containing the JSON representation of a `DogRequest`. The response has `bytes response_json` containing a `DogResponse`. Protobuf JSON clients encode these bytes as base64. The schema and generated Rust client are available in `dog-transport/proto/dog.proto` and `dog_transport::grpc::proto`.

For a command-line session:

```sh
printf '%s\n' '{"request_id":"demo","transport":"Cli","service":"echo","method":"Create","id":null,"tenant":{"tenant_id":"demo"},"params":{},"payload":{"hello":"world"},"metadata":{}}' | cargo run -q -p transport-demo -- cli
```

Each input line produces one JSON response. Invalid input produces an error response and leaves the session usable. Input is limited to 10 MiB per command.

## Application structure

See the complete executable source in `dog-examples/transport-demo/src/main.rs`:

1. Implement `DogService<R, P>` for the business service.
2. Register the service and its hooks with `DogAppBuilder`.
3. Call `build()`.
4. Convert the resulting app with `IntoDogService` and the selected options.
5. Mount the HTTP service in a webserver, run the gRPC server, or run the CLI adapter.

For HTTP, `DogHttpService` implements Tower's `Service` interface. Axum, Actix, and Poem are supported by adapter macros. The blog example uses Poem; the core service does not depend on Poem.

## Authentication and tenant authorization

The echo example is intentionally public and stateless. For private data, use `AuthParams<P>` and an `AuthenticateHook` on every exposed method, including custom methods. `TenantContext` carries a tenant ID; it does not authenticate a user or authorize membership.

Configure a server-held JWT secret, register the JWT/local strategies, install the authentication adapter, and call its `setup(app.clone())` after building the app. `AuthenticationService::from_app` retrieves the installed instance. Access-token verification rejects refresh and identity tokens.

The transport owns the provider field and clears externally supplied trusted auth state. Your business hooks must still authorize which tenant and records the authenticated user may access. Never expose arbitrary TypeQL to an untrusted caller.

## TypeDB

The tested server/driver pairing for this release is TypeDB **3.13.6**. Start a local test server with:

```sh
docker run --rm --name dogrs-typedb -p 127.0.0.1:1729:1729 typedb/typedb:3.13.6
```

This command uses ephemeral container storage. Use a persistent volume, backups, and TLS for a real deployment. TypeDB CE's default local credentials are `admin` / `password`; change them before any public exposure.

`TypeDBAdapter::read`, `write`, and `schema` use their respective transaction types. Schema loading commits discovered schema files together and fails on errors. Run schema changes as explicit, versioned migrations; do not delete or recreate an existing database during application startup.

The live transaction tests create a unique disposable database and clean it up:

```sh
TYPEDB_ADDRESS=127.0.0.1:1729 cargo test -p dog-typedb --test live_database -- --ignored
```

## Queues and deployment

See `dog-queue/README.md` for backend status and persistence requirements. A transport compiling successfully is not a durability guarantee. Payment webhooks require a durable inbox, signature verification, idempotent processing, and reconciliation.

Use the pinned workspace lockfile and run the documented checks before upgrading. The demo binds to loopback. Public deployments need TLS, appropriate request limits, logging, backups, and shutdown handling.
