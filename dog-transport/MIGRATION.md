# Migrating from dog-axum to dog-transport

DogRS now puts shared HTTP behavior in the web-server-agnostic `dog-transport`
crate. Its HTTP implementation uses `http`, `http-body`, and Tower, not Axum.
Axum is one supported host. Other servers need compatible Tower support or an
adapter for their request/response types. This does not require changing your
DogRS services or choosing a particular database or authentication backend.

`dog-axum` 0.2 is deprecated as an integration choice and retained as a
compatibility wrapper. Its REST routes and custom/OAuth dispatch use
`dog-transport`; new integrations should depend on `dog-transport` directly.
Deprecation is documented rather than emitted as compiler warnings so existing
consumers using `-D warnings` can migrate incrementally. Removal is reserved for
a future breaking release; no removal date is set.

## Dependencies

Replace `dog-axum` with:

```toml
[dependencies]
dog-core = "0.2"
dog-transport = { version = "0.2", features = ["http"] }
axum = "0.8" # Your choice of HTTP server, not a dog-transport requirement.
serde_json = "1"
tower = { version = "0.5", features = ["util"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "signal"] }
```

These versions describe the 0.2 source release; use matching Git/path dependencies
until it is published. Enabling `http` does not enable gRPC, CLI, or Iroh.

## Replace the AxumApp wrapper

Previously, `dog_axum::axum(app).use_service("/items", service).listen(address)`
registered and mounted a service. Register it on `DogApp`, then host the transport
with the server you choose:

```rust,no_run
use axum::Router;
use dog_core::DogApp;
use dog_transport::{http::RestParams, HttpOptions, IntoDogService};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app: DogApp<Value, RestParams> = DogApp::default();
    // Register your existing Arc<dyn DogService<Value, RestParams>>:
    // app.register_service("items", items_service);
    let transport = app.into_service(HttpOptions::new().body_limit(1024 * 1024));
    let router = Router::new().fallback_service(transport);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; })
        .await?;
    Ok(())
}
```

The fallback above exposes registered services by the first path segment:
`GET /items`, `POST /items`, and item routes such as `GET /items/123`.
It is appropriate only when that registry is intended to be reachable over HTTP.
For a single aliased service, use an explicit host route instead:

```rust
use axum::{body::Body, extract::{OriginalUri, Path}, http::Request, routing::get, Router};
use dog_core::{DogApp, DogMethod};
use dog_transport::http::{DogHttpService, HttpRoute, RestParams};
use serde_json::Value;
use tower::ServiceExt;

let app: DogApp<Value, RestParams> = DogApp::default();
// app.register_service("users", users_service);
let transport = DogHttpService::new(app, Default::default());
let router: Router = Router::new().route("/api/people/{id}", get(
    move |OriginalUri(uri): OriginalUri, Path(id): Path<String>, mut req: Request<Body>| {
        let transport = transport.clone();
        async move {
            *req.uri_mut() = uri;
            req.extensions_mut().insert(HttpRoute {
                service: "users".into(), method: DogMethod::Get, id: Some(id),
            });
            transport.oneshot(req).await.unwrap().map(Body::new)
        }
    }
));
```

`HttpRoute` is a trusted server-side extension, never a client-supplied routing
instruction. It preserves the original URI and the host router's decoded ID.
`HttpOptions::route("/people", "users")` is also available for simple first-segment
aliases. It is not a multi-segment mount or an authorization allowlist.

## Parameters, authentication, and middleware

| Old API | Replacement |
| --- | --- |
| `dog_axum::params::RestParams` | `dog_transport::http::RestParams` (the old path re-exports it) |
| `FromRestParams` | Not required by transport; parameter types use Serde deserialization |
| `AxumApp::use_service` | `DogApp::register_service` plus a host route or fallback |
| `use_middleware` / `use_service_with` | Native router/Tower layers applied after mounting routes |
| `AxumApp::listen` | Your server's listener and graceful shutdown API |
| Custom REST/OAuth dispatch helpers | `dog_transport::http::call_custom`, or explicit `HttpRoute` |
| WebSocket/SSE helpers | `dog-transport` realtime adapters and explicit authorization callbacks |

`dog_auth::hooks::authenticate::AuthParams<RestParams>` works through the shared
HTTP conversion, including its nested `inner` parameters. Continue installing
authentication and authorization hooks. Neither transport nor the compatibility
wrapper authenticates a caller merely because they supplied `x-tenant-id`.
Validate that the authenticated identity may access the requested tenant/service.

The transport defaults to a 10 MiB request-body limit and a 30-second body-read
budget and dispatch budget (separate budgets). Set `HttpOptions` explicitly to
match your service. A timeout does not prove that an external write was undone.

## Custom methods and OAuth

`http::call_custom` takes the app, service and method names, an
`http::Request<Option<R>>`, and `HttpOptions`. Put the actual HTTP method, URI
(including encoded query), and headers on that request. It uses the same REST
parameter conversion and application deadline as the Tower service, and returns
a `Result<serde_json::Value, DogError>`.

Keep OAuth provider configuration, state/PKCE verification, secure cookies, and
redirect policy in your application/auth layer. Convert successful login results
to the hosting server's redirect type and callback results to JSON. The old
`dog-axum` helpers continue to delegate through this shared custom-call API while
you migrate. Debug callback capture helpers are not a production OAuth flow.

## Upload migration and compatibility changes

Multipart parsing is not part of the generic DogRS wire protocol. New applications
should use their server's multipart extractor with explicit size limits and their
chosen blob storage; pass the resulting application-owned data/reference to the
DogRS service. There is no mandatory storage provider.

The deprecated `dog-axum` upload helper remains only for migration. Its repaired
behavior differs from the old implementation:

- It preserves method, URI, headers, and request extensions.
- Total and per-field limits are enforced while consuming the stream, with a
  30-second conversion deadline. Defaults
  are now 10 MiB; a `None` total limit retains a 200 MiB safety ceiling.
- `TempFile` is the explicit default, preserving the old BlobRef-shaped output.
  Temporary files exist only while the downstream handler runs and are removed
  on success, failure, or cancellation. Copy/move/open them in that handler if
  needed later; a returned path is not durable storage.
- `Base64`, `Metadata`, and `Skip` are now honored; skipped fields become null.
  Base64 data uses the `data` key. Processor closures survive cloning and run;
  processor errors return a generic 500. The FieldProcessor alias now uses Arc.
- Duplicate field names and more than 1,024 fields are rejected. Missing MIME
  types are rejected when a content-type allowlist is configured. MIME metadata
  is supplied by the client and is not file-content validation.
- Fields are buffered within configured limits; this is not a zero-copy uploader.
  Raw form values and internal file paths are no longer printed to logs.

The wrapper also preserves pending middleware when cloned and when adding a
service-specific layer. HTTP errors, parsing, payload limits and timeouts now
follow `dog-transport`. Review response-message differences when migrating clients.
