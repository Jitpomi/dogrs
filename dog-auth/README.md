# dog-auth

Core authentication module for DogRS, inspired by FeathersJS authentication while staying transport-agnostic and avoiding vendor lock-in.

## What you get

- **`AuthenticationService<P>`**
  - Installed in `DogApp` state under `"authentication"`
  - Registers and runs strategies
  - Creates JWT access tokens (when JWT features are enabled)
- **Strategies**
  - JWT (`JwtStrategy`) in this crate
  - Local and OAuth strategies live in companion crates (`dog-auth-local`, `dog-auth-oauth`)
- **Hooks**
  - `AuthenticateHook` for protecting service methods via before-hooks
  - Connection + event hook stubs to mirror Feathers-like flows
- **`AuthServiceAdapter<P>`**
  - A `DogService<Value, P>` adapter that exposes only:
    - `create` (login)
    - `remove` (logout)
  - Designed to be mounted as an external `/auth` endpoint by server adapters (HTTP/WebSocket/etc.).

## Install

In your crate:

```toml
[dependencies]
dog-auth = { path = "../dog-auth" }
```

Enable a JWT backend feature (one of):

- `jwt-aws-lc-rs`
- `jwt-rust-crypto`

## Configuration

`AuthOptions` is stored in app state under `"authentication.options"`.

Key options:

- **`jwt.secret`**
  - Required if `AuthStrategy::Jwt` is enabled
- **`strategies`**
  - Enabled strategy list (e.g. `Jwt`, `OAuth`, `Custom("local")`)
- **Entity attachment (Feathers-like)**
  - `entity`: JSON key to attach the entity under (e.g. `"user"`)
  - `service`: service name used for entity loading (e.g. `"users"`)
  - `entity_id_claim`: JWT claim containing the entity id (defaults to `"sub"` if not set)

JWT payload default:

- When `entity` is configured, `AuthenticationService::create(...)` will automatically include the
  authenticated entity's `id` in the JWT payload under `entity_id_claim` (default `"sub"`) unless
  the caller already provided that claim.

## Basic usage

### 1) Create + install the service

```rust
use std::sync::Arc;
use dog_auth::{AuthenticationService, AuthOptions};
use dog_core::DogAppBuilder;
use serde_json::Value;

// P is your params type
fn setup_auth<P: Send + Clone + 'static>(builder: &mut DogAppBuilder<Value, P>) -> anyhow::Result<()> {
    let options = AuthOptions::default();
    
    // Create the auth builder
    let mut auth_builder = AuthenticationService::builder(builder, Some(options))?;

    // Register strategies here (JWT + any from other crates)
    // auth_builder.register_strategy("jwt", Arc::new(JwtStrategy::new()));

    // Build and install the auth service into the DogAppBuilder
    let auth = Arc::new(AuthenticationService::new(Arc::new(auth_builder.build())));
    AuthenticationService::install(builder, auth);

    Ok(())
}
```

### 2) Protect services with `AuthenticateHook`

`AuthenticateHook` is a DogRS before-hook. It checks:

- `params.provider()`
  - If missing/empty, the call is treated as internal and allowed through
- `params.authentication()` or `Authorization: Bearer ...`

```rust
use dog_auth::hooks::{AuthenticateHook, AuthParams};

// Construct hook
// let hook = AuthenticateHook::<AuthParams<MyParams>>::new(vec!["jwt".into()]);
```

### 3) Expose an external `/auth` endpoint with `AuthServiceAdapter`

`AuthServiceAdapter<P>` is a thin wrapper around `AuthenticationService<P>` that implements
`DogService<Value, P>` so it can be mounted by server adapters.

```rust
use std::sync::Arc;
use dog_auth::{AuthServiceAdapter, AuthenticationService};
use dog_core::{DogAppBuilder, DogService};
use serde_json::Value;

fn mount_auth<P: Send + Sync + Clone + 'static>(builder: &mut DogAppBuilder<Value, P>, auth: Arc<AuthenticationService<P>>) -> anyhow::Result<()> {
    let adapter = Arc::new(AuthServiceAdapter::new(auth));
    builder.register_service("auth", adapter);
    Ok(())
}
```

## Notes

- `dog-auth` is **transport-agnostic**. HTTP/WebSocket concerns belong in the server adapter.
- If you use entity attachment, ensure your `DogService` implementation supports the required operations for your strategy.
