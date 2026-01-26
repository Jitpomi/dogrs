






# dog-axum

A high-level REST framework built on Axum that provides service-oriented architecture with pluggable middleware support.

## Features

- **Service-oriented REST API** - Clean separation between routes and business logic
- **Pluggable middleware** - Apply middleware per service or globally
- **Multipart upload support** - Built-in middleware for handling file uploads with BlobRef pattern
- **Framework-safe patterns** - Memory-efficient handling of large files
- **Tower ecosystem integration** - Full compatibility with Tower middleware

## Optional integration features

### `auth`

Enable `dog-axum`'s `auth` feature to integrate with `dog-auth` hook params.

This adds `FromRestParams` support for:

- `dog_auth::hooks::authenticate::AuthParams<dog_axum::params::RestParams>`

so application code can simply use:

```rust
pub type Params = dog_auth::hooks::authenticate::AuthParams<dog_axum::params::RestParams>;
```

without writing boilerplate conversion code.

### OAuth DX helpers

`dog-axum` includes small, provider-agnostic helpers that make it easier to expose OAuth flows over HTTP.

REST helpers (in `dog_axum::rest`):

- `call_custom_json` / `call_custom_redirect`
- `call_custom_json_q` / `call_custom_json_qd`
- `call_custom_redirect_q` / `call_custom_redirect_qd`
- `call_custom_redirect_location` (defaults `location_key` to `"location"`)
- `oauth_callback_capture_typed` (standard capture response for service-mode callback testing)

Route builder (in `dog_axum::oauth`):

- `OAuthRoutes`
- `mount_oauth_routes`

These helpers are intentionally policy-free: you provide the service name, custom method names, paths, and callback payload shape.

## Quick Start

### 1. Basic REST Server

Create a minimal dog-axum server:

```rust
use dog_axum::AxumApp;
use dog_core::{DogApp, DogService};
use std::sync::Arc;

// Define your request/response types
#[derive(serde::Deserialize)]
struct CreateUserRequest {
    name: String,
    email: String,
}

#[derive(serde::Serialize)]
struct User {
    id: u32,
    name: String,
    email: String,
}

// Create a service
struct UserService;

#[async_trait::async_trait]
impl DogService<CreateUserRequest, ()> for UserService {
    async fn create(&self, _tenant: TenantContext, data: CreateUserRequest) -> Result<User> {
        Ok(User {
            id: 1,
            name: data.name,
            email: data.email,
        })
    }
}

// Build the server
#[tokio::main]
async fn main() -> Result<()> {
    let app = DogApp::new();
    let user_service = Arc::new(UserService);
    
    let server = AxumApp::new(app)
        .use_service("/users", user_service)
        .service("/health", || async { "ok" });
    
    server.listen("0.0.0.0:3030").await
}
```

### 2. Adding Middleware

Apply middleware to specific services:

```rust
use dog_axum::{AxumApp, middlewares::MultipartToJson};
use tower::ServiceBuilder;

let server = AxumApp::new(app)
    // Single middleware
    .use_service_with("/upload", upload_service, MultipartToJson::default())
    
    // Multiple middleware with ServiceBuilder
    .use_service_with("/api", api_service,
        ServiceBuilder::new()
            .layer(CorsMiddleware::permissive())
            .layer(AuthenticationMiddleware::new())
            .layer(RateLimitingMiddleware::new(100))
    )
    
    // Service without middleware
    .use_service("/health", health_service);
```

### 3. Global Middleware

Apply middleware to all routes:

```rust
let mut server = AxumApp::new(app)
    .use_service("/users", user_service)
    .use_service("/posts", post_service);

// Add global middleware to the router
server.router = server.router
    .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
    .layer(tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
    );
```

## Middleware

### Built-in Middleware

#### MultipartToJson

Handles multipart form uploads and converts them to JSON with BlobRef pattern for files:

```rust
use dog_axum::middlewares::{MultipartToJson, MultipartConfig};

let config = MultipartConfig::default()
    .with_max_file_size(50 * 1024 * 1024)  // 50MB per file
    .with_max_total_size(200 * 1024 * 1024); // 200MB total

let server = AxumApp::new(app)
    .use_service_with("/upload", upload_service, 
        MultipartToJson::with_config(config)
    );
```

**BlobRef Pattern**: Files are streamed to temporary storage and services receive references:

**Visual BlobRef Flow:**

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  Multipart      │    │  MultipartToJson│    │  Temp Storage   │    │    Service      │
│  Upload         │───▶│  Middleware     │───▶│  /tmp/file_*    │───▶│  Gets BlobRef   │
│  (7MB file)     │    │                 │    │                 │    │  (not raw data) │
└─────────────────┘    └─────────────────┘    └─────────────────┘    └─────────────────┘
                                │                        │                        │
                                ▼                        ▼                        ▼
                       ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
                       │ Streams chunks  │    │ File written to │    │ JSON with       │
                       │ (no memory      │    │ disk (not RAM)  │    │ file reference  │
                       │ buffering)      │    │                 │    │ (framework-safe)│
                       └─────────────────┘    └─────────────────┘    └─────────────────┘
```

```rust
// Service receives this JSON structure for file uploads:
{
    "name": "John Doe",
    "avatar": {
        "key": "temp/uuid",
        "temp_path": "/tmp/multipart_file_uuid", 
        "filename": "avatar.jpg",
        "content_type": "image/jpeg",
        "size": 1024000
    }
}

// Your service can then process the file:
#[derive(serde::Deserialize)]
struct CreateUserRequest {
    name: String,
    avatar: Option<BlobRef>,
}

#[derive(serde::Deserialize)]
struct BlobRef {
    temp_path: String,
    filename: Option<String>,
    content_type: Option<String>,
    size: u64,
}
```

### Custom Middleware

Create custom middleware using Tower patterns:

```rust
use tower::{Layer, Service};
use axum::{response::Response, body::Body};

#[derive(Clone)]
pub struct LoggingMiddleware;

impl<S> Layer<S> for LoggingMiddleware {
    type Service = LoggingService<S>;
    
    fn layer(&self, inner: S) -> Self::Service {
        LoggingService { inner }
    }
}

#[derive(Clone)]
pub struct LoggingService<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for LoggingService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;
    
    fn call(&mut self, req: Request<Body>) -> Self::Future {
        println!("Request: {} {}", req.method(), req.uri());
        let future = self.inner.call(req);
        Box::pin(async move {
            let response = future.await?;
            println!("Response: {}", response.status());
            Ok(response)
        })
    }
}
```

### Middleware Execution Order

Middleware executes in **reverse order** of how it's added:

```rust
ServiceBuilder::new()
    .layer(CorsMiddleware::new())        // Executes 1st (outermost)
    .layer(AuthMiddleware::new())        // Executes 2nd  
    .layer(RateLimitMiddleware::new())   // Executes 3rd
    .layer(MultipartToJson::default())   // Executes 4th (innermost)
```

**Visual Request Flow:**

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐    ┌──────────────┐    ┌─────────────┐
│   Request   │───▶│ CORS Layer   │───▶│ Auth Layer  │───▶│ RateLimit    │───▶│ Multipart   │
│  (Client)   │    │ (Outermost)  │    │             │    │ Layer        │    │ Layer       │
└─────────────┘    └──────────────┘    └─────────────┘    └──────────────┘    └─────────────┘
                                                                                       │
                                                                                       ▼
┌─────────────┐    ┌──────────────┐    ┌─────────────┐    ┌──────────────┐    ┌─────────────┐
│  Response   │◀───│ CORS Layer   │◀───│ Auth Layer  │◀───│ RateLimit    │◀───│   Service   │
│  (Client)   │    │              │    │             │    │ Layer        │    │ (Business)  │
└─────────────┘    └──────────────┘    └─────────────┘    └──────────────┘    └─────────────┘
```

## Examples

See the `dog-examples/` directory for complete examples:

- **music-blobs**: File upload service with multipart middleware
- **blog-axum**: Basic REST API with CRUD operations
- **social-typedb**: Complex service with authentication middleware

## Dependencies

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-axum = "0.1.0"
dog-core = "0.1.0"
axum = "0.7"
tower = "0.4"
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
```

## Architecture

dog-axum provides a service-oriented architecture where:

- **Services** implement business logic (`DogService` trait)
- **Routes** are automatically generated for CRUD operations
- **Middleware** can be applied per-service or globally
- **Request/Response** types are strongly typed with serde

**Visual Architecture:**

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                                  dog-axum                                           │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                     │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
│  │   Client    │───▶│   Router    │───▶│ Middleware  │───▶│   Service   │         │
│  │ (HTTP Req)  │    │ (Axum)      │    │ (Tower)     │    │ (Business)  │         │
│  └─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘         │
│                             │                   │                   │              │
│                             ▼                   ▼                   ▼              │
│                    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
│                    │ Auto Routes │    │ Per-Service │    │ Typed Data  │         │
│                    │ GET/POST/   │    │ or Global   │    │ Serde JSON  │         │
│                    │ PUT/DELETE  │    │ Layers      │    │ Validation  │         │
│                    └─────────────┘    └─────────────┘    └─────────────┘         │
│                                                                                     │
├─────────────────────────────────────────────────────────────────────────────────────┤
│                              Built on Tower + Axum                                 │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

**Service Registration Flow:**

```
AxumApp::new(app)
    │
    ├─ .use_service("/users", user_service)
    │   └─ Creates: GET/POST/PUT/DELETE /users routes
    │
    ├─ .use_service_with("/upload", upload_service, MultipartToJson)
    │   └─ Creates: Routes + applies middleware to this service only
    │
    └─ .service("/health", health_handler)
        └─ Creates: Custom route with handler function
```

This separation allows for clean, testable code with flexible middleware composition.
