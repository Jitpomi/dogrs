# dog-transport

Pluggable transport adapters for the DogRS framework. This crate provides uniform abstractions for exposing your transport-agnostic services over HTTP (Axum/Tower), WebSockets, gRPC, and others.

By separating the network layer from your core logic, the exact same `DogApp` service registry can be exposed across multiple protocols simultaneously without modifying your business code.

## Abstractions

### The `IntoDogService` Trait
The core trait used to transform a `DogApp` instance into a protocol-specific service:

```rust
pub trait IntoDogService<T> {
    type Service;

    fn into_service(self, transport: T) -> Self::Service;
}
```

By implementing this trait, different protocol adapters (such as HTTP or gRPC) can consume a `DogApp` and configuration options to build a server-ready dispatcher.

---

## Supported Transports

### 1. HTTP (via Axum / Tower)
Exposes services as standard REST endpoints using `DogHttpService` which implements `tower::Service`.

**Feature required:** `features = ["http"]`

#### Configuration Options (`HttpOptions`)
*   `request_id_header`: Custom HTTP header to extract/propagate request IDs (default: `"x-request-id"`).
*   `tenant_header`: Custom HTTP header to identify the tenant context (default: `"x-tenant-id"`).
*   `body_limit`: Maximum request payload size allowed in bytes (default: `10MB`).
*   `enable_cors`: Quick flag to set permissive CORS headers on responses.
*   `route(path, service)`: Maps an incoming URL path prefix framework-agnostically to a target service name in the registry (e.g. `.route("/persons", "persons")`).

#### Service Routing Patterns

You can expose and route HTTP requests to your DogRS services using two main patterns:

##### Pattern A: Uniform Global Routing (Framework Agnostic)
Register all your routing mappings directly on `HttpOptions`. Then, mount the main `http_service` once as a single fallback or default route in your framework.

```rust
use dog_core::DogApp;
use dog_transport::{HttpOptions, IntoDogService};
use axum::{routing::get, Router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dog_app = DogApp::builder().build();

    // Configure all route mappings framework-agnostically:
    let http_service = dog_app.clone().into_service(
        HttpOptions::default()
            .tenant_header("x-tenant-id")
            .route("/persons", "persons")
            .route("/communities", "communities")
    );

    // Mount once as the fallback/default route
    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .fallback_service(http_service);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, router).await?;
    Ok(())
}
```

##### Pattern B: Scoped Service Routing (Native Framework Routers)
Bind individual `DogHttpService` instances to specific registry services using the `.service("name")` method. This creates a scoped service instance that bypasses path-based service parsing, routing all requests directly to the target service. You then mount each scoped service natively under its path prefix:

```rust
    let http_service = dog_app.clone().into_service(HttpOptions::default());

    let router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest_service("/persons", http_service.service("persons"))
        .nest_service("/communities", http_service.service("communities"));
```

### Routing Mechanics & Path Resolution

In standard Axum/Tower configurations, routing subpaths is often done with explicit wildcards like `.route_service("/persons/*path", service)`. DogRS simplifies this process using Axum's `.nest_service(path, service)` and framework-agnostic scoped paths.

#### Why `nest_service` Instead of `route_service` with Wildcards?
*   **Automatic Prefix Matching:** Axum's `.nest_service("/prefix", service)` matches both the exact path `/prefix` and any subpaths (e.g., `/prefix/123`). This removes the need to define multiple routes or use explicit wildcard matching patterns (like `/*path`).
*   **Path Prefix Stripping:** When nesting a service, Axum automatically strips the prefix from the request path before it reaches the nested service. For example, a request to `GET /persons/123` mounted under `.nest_service("/persons", service)` is received by the service with a URI path of `/123`.
*   **Decoupled Service Logic:** Stripping the prefix means the underlying `DogHttpService` does not need to know its external mount point (e.g., `/api` or `/persons`). It only acts on the relative subpath, ensuring maximum portability.

#### Path Parsing under Scoped Services
Because `nest_service` strips the prefix, `DogHttpService` performs path parsing dynamically to ensure it works correctly whether the request path prefix is stripped or not:
*   **With Fixed Service Scoping (`.service("persons")`):** 
    *   If the path is `/` (meaning the prefix `/persons` was stripped by the framework), the ID is resolved to `None` (triggers a `find` or `create` operation).
    *   If the path is `/123` (prefix stripped), the first segment is not the service name, so it is treated directly as the resource ID `"123"` (triggers a `get`, `update`, `patch`, or `remove` operation).
    *   If the path is `/persons/123` (prefix not stripped, e.g., when integrating with frameworks that do not strip prefixes), the service matches its fixed service name and extracts `"123"` as the ID.
*   **Without Scoping (Global Registry):** 
    *   The first path segment is always interpreted as the service name (e.g., `/persons/123` $\to$ service `"persons"`, ID `"123"`).

### 2. gRPC (`grpc` feature)

`app.into_service(GrpcOptions::new())` returns `DogGrpcService`. Mount its
`into_server()` result in a Tonic server, or use `serve_with_shutdown`.
Reflection is opt-in through `enable_reflection(true)`. The versioned protocol is
`dog.v1.DogTransport/Call`; it carries JSON `DogRequest` and `DogResponse` bytes.
The crate exports the generated client, server, and reflection descriptor under
`dog_transport::grpc::proto`. Messages are limited to 10 MiB. Configure TLS in
Tonic or at a trusted reverse proxy before exposing a listener publicly.

### 3. CLI (`cli` feature)

`app.into_service(CliOptions::new()).run_stdio().await` reads one JSON
`DogRequest` per input line and writes one response per output line. Invalid
commands produce structured errors without terminating the session; oversized
commands terminate it. `run(reader, writer)` supports embedding and tests.
`interactive(true)` writes usage guidance to stderr; stdout remains machine-readable.

### 4. Iroh (`iroh` feature)

`app.into_service(IrohOptions::new(alpn)).await?` asynchronously creates a Router.
Endpoint creation errors are returned; setup does not block the async runtime.
Use `.endpoint(endpoint)` to share an existing endpoint, or
`app.into_service((router_builder, alpn))` to compose multiple protocols before
spawning the router. Requests and responses use length-prefixed JSON frames,
with a 10 MiB frame limit and a 30-second request-read timeout.

`BlobPayloadAdapter` stores/retrieves serialized payloads in an Iroh blob store;
it is separate from the RPC transport and does not automatically fetch remote blobs.

### WebSockets and SSE

Axum adapter macros exist under the `http` feature. RPCs use the same hooks as
other transports. Automatic global WebSocket broadcasts are disabled by default;
set `ws.public_broadcasts` to the string `"true"` only for intentionally public
channels. Private streams require application-authorized, tenant-scoped channels.
Mount SSE handlers behind the application's authorization middleware.
`WebSocketOptions` and `SseOptions` now apply to the adapters. See
[realtime operation and migration](REALTIME.md) for authorization, limits,
disconnect behavior and executable test coverage.

HTTP, gRPC, CLI and Iroh application dispatch now has a default 30-second
deadline. Set `request_timeout_secs` on the corresponding options to change it
(1–86,400 seconds). HTTP body collection has a separate deadline of the same
duration. CLI still waits for interactive input, and Iroh retains its separate
frame read/write deadlines. Timeout cancels cooperative handlers; it does not
undo external effects, so reconcile uncertain outcomes before retrying mutations.

See the runnable `transport-demo` example and `docs/quickstart.md`.

---

## Under the Hood: Protocol Mapping

When using the `http` transport, incoming HTTP requests are parsed into a uniform `DogRequest`:
1.  **Request ID extraction**: Taken from the configured request ID header (or generated as a new UUID).
2.  **Tenant context**: Derived from the configured tenant header (falls back to `"default"`).
3.  **Service routing**: Inferred from the path (e.g., `/users/123` maps to the `"users"` service).
4.  **Method mapping**:
    *   `GET` (no ID) $\to$ `DogMethod::Find`
    *   `GET` (with ID) $\to$ `DogMethod::Get`
    *   `POST` $\to$ `DogMethod::Create`
    *   `PUT` $\to$ `DogMethod::Update`
    *   `PATCH` $\to$ `DogMethod::Patch`
    *   `DELETE` $\to$ `DogMethod::Remove`
    *   Custom methods can be invoked by setting the `x-service-method` header (e.g., `x-service-method: custom_rpc`).
5.  **Parameters serialization**: Extracts query strings, URI paths, and HTTP headers into `DogParams`.
6.  **Dispatch**: Calls `DogApp::handle(request)` and formats the output into a JSON HTTP response or a sanitized `DogError` shape on failure.

---

## Framework Independence (Actix-web, Poem, Salvo, etc.)

`DogHttpService` is built on top of standard `http::Request` and `http::Response` types and implements Tower's `Service` trait. This makes it completely decoupled from any single web framework.

### Poem
Poem has native Tower integration. You can mount a scoped HTTP service using Poem's built-in `TowerService` adapter:

```rust
use poem::{Route, nest};

let http_service = dog_app.into_service(HttpOptions::default());

let app = Route::new()
    .nest("/persons", poem::tower_compat::TowerService::new(http_service.service("persons")))
    .nest("/communities", poem::tower_compat::TowerService::new(http_service.service("communities")));
```

### Actix-web
Because Actix-web defines its own service trait and HTTP types, a small request-mapping wrapper is used to bridge types:

```rust
use actix_web::{web, HttpRequest, HttpResponse, Responder};

async fn handle_dog_request(
    req: HttpRequest,
    body: web::Bytes,
    http_service: web::Data<DogHttpService<R, P>>
) -> impl Responder {
    // 1. Map Actix request/body into standard http::Request
    let http_req = convert_actix_to_http_request(req, body);
    
    // 2. Call the pre-bound Tower service
    let http_res = http_service.call(http_req).await.unwrap();
    
    // 3. Map standard http::Response back to Actix HttpResponse
    convert_http_to_actix_response(http_res)
}

// Mount the route:
App::new()
    .app_data(web::Data::new(http_service.service("persons")))
    .service(web::scope("/persons").default_service(web::to(handle_dog_request)))
```
