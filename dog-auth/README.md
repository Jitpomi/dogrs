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

## Basic usage

### 1) Create + install the service

```rust
use std::sync::Arc;
use dog_auth::{AuthenticationService, AuthOptions};
use dog_core::DogApp;
use serde_json::Value;

// P is your params type
fn setup_auth<P: Send + Clone + 'static>(app: DogApp<Value, P>) -> anyhow::Result<Arc<AuthenticationService<P>>> {
    let options = AuthOptions::default();
    let auth = Arc::new(AuthenticationService::new(app.clone(), Some(options))?);

    AuthenticationService::install(&app, auth.clone());

    // Register strategies here (JWT + any from other crates)
    // auth.register_strategy("jwt", Arc::new(JwtStrategy::new(&auth.base)));

    Ok(auth)
}
```

### 2) Protect services with `AuthenticateHook`

`AuthenticateHook` is a DogRS before-hook. It checks:

- `params.provider()`
  - If missing/empty, the call is treated as internal and allowed through
- `params.authentication()` or `Authorization: Bearer ...`

```rust
use dog_auth::hooks::{AuthenticateHook, AuthParams};

// Construct from app state
// let hook = AuthenticateHook::<AuthParams<MyParams>>::from_app(&app, vec!["jwt".into()])?;
```

## Notes

- `dog-auth` is **transport-agnostic**. HTTP/WebSocket concerns belong in the server adapter.
- If you use entity attachment, ensure your `DogService` implementation supports the required operations for your strategy.
