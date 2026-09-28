# Auth Demo (`auth-demo`)

A local learning example demonstrating how to implement authentication in DogRS using `dog-auth` and `dog-transport`.

This demo showcases how to set up an immutable `DogAppBuilder`, configure multiple authentication strategies (Local, JWT, and Google OAuth2), and decouple your HTTP routing from your internal service registry.

## Features

- **Immutable `DogAppBuilder`**: Lock-free, high-performance dependency injection.
- **Multiple Strategies**:
  - `local`: Username and password authentication using `dog-auth-local`.
  - `jwt`: Stateless token-based authentication.
  - `oauth2`: Google OAuth login via `dog-auth-oauth`.
- **Decoupled Routing**: Uses `use_service_as` to map the clean `/auth` REST path to the internal `"authentication"` service.
- **Schema Validation**: Uses `dog-schema` to enforce payload validation on user creation.

## Getting Started

### Prerequisites

Set `AUTH_JWT_SECRET` for local/JWT authentication. Configure the three Google settings together only if you want OAuth; otherwise leave all three unset.

```env
HTTP_PORT=3000
# Generate a random secret, e.g. openssl rand -hex 32
AUTH_JWT_SECRET=<your-random-secret-at-least-32-bytes>
# Optional Google OAuth settings:
GOOGLE_CLIENT_ID=your-google-client-id
GOOGLE_CLIENT_SECRET=your-google-client-secret
GOOGLE_REDIRECT_URL=http://localhost:3000/oauth/google/callback
```

### Running the Server

Start the application:

```bash
cargo run -p auth-demo
```

The server will bind to `http://127.0.0.1:3000`.

## API Examples

### 1. Create a User

To authenticate, you first need a user in the system. The `users` service uses `dog-schema` to validate that `username` and `password` are provided.

```bash
curl -i -X POST http://127.0.0.1:3000/users \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser", "password":"password123"}'
```

### 2. Login (Local Strategy)

Send your credentials to the `/auth` endpoint to receive a JWT `accessToken`. Notice that we use the `local` strategy.

```bash
curl -i -X POST http://127.0.0.1:3000/auth \
  -H "Content-Type: application/json" \
  -d '{
    "strategy": "local",
    "username": "testuser",
    "password": "password123"
  }'
```

**Response:**
```json
{
  "accessToken": "eyJ0eXAiOi...",
  "authentication": { "strategy": "local" },
  "user": { "id": "user_123", "username": "testuser" }
}
```

### 3. Access Protected Routes (JWT Strategy)

Use the returned `accessToken` to hit protected routes, such as creating a new message.

```bash
curl -i -X POST http://127.0.0.1:3000/messages \
  -H "Authorization: Bearer eyJ0eXAiOi..." \
  -H "Content-Type: application/json" \
  -d '{"text":"Hello, DogRS!", "sender":"user_123"}'
```

### 4. OAuth2 (Google)

To authenticate via Google, simply open your browser and navigate to:

```
http://127.0.0.1:3000/oauth/google
```

The framework will handle the redirect to Google, process the callback, create the user if they don't exist, and return a standard `AuthenticationResult` with a valid JWT access token.

## Architectural Highlights

### Decoupled Routing

In `src/app.rs`, the router is configured natively with Axum:

```rust
    let http_service = dog_app.clone().into_service(
        HttpOptions::default()
            .tenant_header("x-tenant-id")
            .enable_cors(true)
    );

    let router = Router::new()
        .fallback_service(http_service);
```

By default, the HTTP service automatically maps external paths directly to their corresponding registered services, routing `/authentication` (or `/auth` if registered as such) to the authentication backend. We use standard Axum routing and fallbacks to pass incoming HTTP requests directly to our core service engine.

### No Duplicate Adapters

In `src/services/mod.rs`, the `configure` function takes the `auth_adapter` built during the strategy initialization phase instead of creating a new one. This prevents duplicate instances from being registered, ensuring that the `setup(dog_app)` method properly wires the router to the initialized application state.

This demo uses process-local user and OAuth state storage. It is not a production identity service. A local durable `FileTokenStore` now supplies server-side revocation and atomic refresh consumption. Set `AUTH_TOKEN_STORE_DIR` to a trusted local directory (default `.dogrs-auth-tokens`). Multiple processes may share that directory on a local filesystem; distributed hosts need a shared transactional `TokenStore`. Revocation markers are retained intentionally: archive them only offline after all previously issued tokens expire. Users and pending OAuth state remain in memory. See [auth hardening](../../docs/auth-hardening.md).

The server binds only to loopback. The local file token store is an application adapter, not a framework storage requirement. See the [example coverage guide](../README.md).

Google OAuth is optional: omit all three Google settings to run local/JWT authentication without an external account. Partial Google configuration fails startup. Logout uses the configured durable revocation store; refresh-token APIs are available through the authentication core.

The example uses only the `default` tenant. User reads and mutations are restricted to the authenticated account; OAuth identity fields are server-managed. Messages form an authenticated shared board, not private mail: only the author may change or delete a message.
