# Auth Demo (`auth-demo`)

A complete, production-ready example demonstrating how to implement authentication in DogRS using `dog-auth` and `dog-axum`.

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

You need a `.env` file (or exported environment variables) for Google OAuth to work. If you don't need OAuth, you can skip this, but the OAuth routes will fail to initialize.

```env
HTTP_PORT=3000
AUTH_JWT_SECRET=super-secret-key
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
  -d '{"text":"Hello, DogRS!"}'
```

### 4. OAuth2 (Google)

To authenticate via Google, simply open your browser and navigate to:

```
http://127.0.0.1:3000/oauth/google
```

The framework will handle the redirect to Google, process the callback, create the user if they don't exist, and return a standard `AuthenticationResult` with a valid JWT access token.

## Architectural Highlights

### Decoupled Routing (`use_service_as`)

In `src/app.rs`, you'll notice we mount the authentication service like this:

```rust
ax = ax
    .use_service("/messages", svcs.messages)
    .use_service("/users", svcs.users)
    .use_service_as("/auth", "authentication", svcs.auth_svc)
    .use_service("/oauth", svcs.oauth);
```

By default, `dog-auth` registers the internal service as `"authentication"` (following the FeathersJS convention). Using `use_service_as` allows us to decouple the external HTTP path (`/auth`) from the internal core registry (`"authentication"`), giving us beautiful URLs without compromising the core architecture.

### No Duplicate Adapters

In `src/services/mod.rs`, the `configure` function takes the `auth_adapter` built during the strategy initialization phase instead of creating a new one. This prevents duplicate instances from being registered, ensuring that the `setup(dog_app)` method properly wires the router to the initialized application state.
