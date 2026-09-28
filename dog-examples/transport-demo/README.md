# Transport demo

A single public, stateless `echo` service composed with `dog-transport`.
No database, credentials or cloud resources are required.

```sh
cargo run -p transport-demo -- http
cargo run -p transport-demo -- grpc
cargo run -p transport-demo -- cli
cargo run -p transport-demo -- iroh
```

HTTP listens on `127.0.0.1:3000`; gRPC on `127.0.0.1:50051` with reflection.
CLI reads NDJSON on stdin. Iroh prints its endpoint ID and uses the ALPN
`dogrs/echo/1`; it may be reachable beyond the local machine. The service accepts
arbitrary public JSON and performs no authenticated or private operations.

`services/echo/` contains the service; `app.rs` composes it and selects a transport.
Server modes shut down on Ctrl-C. See the [quickstart](../../docs/quickstart.md)
for HTTP, gRPC and CLI requests and the [transport guide](../../dog-transport/README.md)
for the Iroh wire protocol, authentication and transport limits.
