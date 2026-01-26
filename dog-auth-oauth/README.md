# dog-auth-oauth

OAuth2 authentication strategy + orchestration helpers for DogRS.

This crate is intentionally **provider-agnostic** and **transport-agnostic**:

- OAuth provider specifics are implemented by you (or adapters) via a trait.
- HTTP redirects / callback endpoints belong in the server adapter.

## What you get

### Strategy

- **`OAuthStrategy<P>`** (implements `dog_auth::core::AuthenticationStrategy`)
  - Validates provider
  - Accepts:
    - `accessToken` (already exchanged)
    - `code` (can be exchanged by a registered provider)
    - optional `profile` (pre-fetched)
  - Optional entity linking (see `OAuthEntityResolver`)

### Provider plugin API

- **`OAuthProvider<P>`**
  - `exchange_code(code, ctx) -> access_token`
  - `fetch_profile(access_token, ctx) -> Option<Value>`

### Entity linking (for custom-only backends/services)

- **`OAuthEntityResolver<P>`**
  - `resolve_entity(provider, profile, ctx) -> Option<Value>`
  - Lets you link/create/load users using **custom service methods** (no `find` required)

### Service orchestrator

- **`OAuthService<P>`**
  - `authenticate_callback(provider, payload, params, ctx, jwt_overrides)`
  - Calls `dog-auth`’s `AuthenticationService::create(...)`
  - Optional redirect resolution via `OAuthRedirect<P>`

## Install

```toml
[dependencies]
dog-auth = { path = "../dog-auth" }
dog-auth-oauth = { path = "../dog-auth-oauth" }
```

### Optional features

#### `oauth2-client`

Enables a small, provider-agnostic helper built on the `oauth2` crate:

- `OAuth2AuthorizationCodeProvider<P>`

This lets you implement common OAuth2 authorization-code providers without wiring the `oauth2` client manually in every app.

```toml
[dependencies]
dog-auth-oauth = { path = "../dog-auth-oauth", features = ["oauth2-client"] }
```

Example:

```rust
use std::sync::Arc;

use dog_auth::AuthenticationService;
use dog_auth_oauth::{OAuth2AuthorizationCodeProvider, OAuthStrategy, OAuthStrategyOptions};

fn register_google_oauth<P: Clone + Send + Sync + 'static>(auth: Arc<AuthenticationService<P>>) -> anyhow::Result<()> {
    let provider = OAuth2AuthorizationCodeProvider::<P>::new(
        "google",
        "client_id",
        "client_secret",
        "https://accounts.google.com/o/oauth2/v2/auth",
        "https://oauth2.googleapis.com/token",
        "http://localhost:3000/oauth/google/callback",
        vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
        Some("https://openidconnect.googleapis.com/v1/userinfo".to_string()),
    )?;

    let mut opts: OAuthStrategyOptions<P> = OAuthStrategyOptions::default();
    opts.default_provider = Some("google".to_string());
    opts.providers.insert("google".to_string(), Arc::new(provider));

    let strategy = OAuthStrategy::new(&auth.base).with_options(opts);
    auth.register_strategy("oauth", Arc::new(strategy));

    Ok(())
}
```

## Registering an OAuth provider

```rust
use std::sync::Arc;
use dog_auth::AuthenticationService;
use dog_auth_oauth::{OAuthProvider, OAuthStrategy};
use dog_core::HookContext;
use serde_json::Value;

struct MyGoogleProvider;

#[async_trait::async_trait]
impl<P> OAuthProvider<P> for MyGoogleProvider
where
    P: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str { "google" }

    async fn exchange_code(&self, code: &str, _ctx: &mut HookContext<Value, P>) -> anyhow::Result<String> {
        // Perform OAuth2 code exchange using whatever HTTP client your adapter uses.
        Ok(code.to_string())
    }

    async fn fetch_profile(&self, _token: &str, _ctx: &mut HookContext<Value, P>) -> anyhow::Result<Option<Value>> {
        Ok(Some(serde_json::json!({ "sub": "provider-user-id" })))
    }
}

fn register_oauth<P: Clone + Send + Sync + 'static>(auth: Arc<AuthenticationService<P>>) {
    let strategy = OAuthStrategy::new(&auth.base)
        .register_provider(Arc::new(MyGoogleProvider));

    auth.register_strategy("oauth", Arc::new(strategy));
}
```

## Using `OAuthEntityResolver` (custom services)

In the snippet below, `TypeDbUserResolver` is only a name to make the example concrete. The same pattern applies to any backend/service that prefers custom methods over CRUD.

```rust
use std::sync::Arc;
use dog_auth_oauth::{OAuthEntityResolver, OAuthStrategy};
use dog_core::HookContext;
use serde_json::Value;

struct TypeDbUserResolver;

#[async_trait::async_trait]
impl<P> OAuthEntityResolver<P> for TypeDbUserResolver
where
    P: Clone + Send + Sync + 'static,
{
    async fn resolve_entity(
        &self,
        provider: &str,
        profile: &Value,
        ctx: &mut HookContext<Value, P>,
    ) -> anyhow::Result<Option<Value>> {
        // Example: call a custom method on your users service.
        // let users = ctx.services.service::<Value, P>("users")?;
        // let out = users.custom(&ctx.tenant, "oauthUpsert", serde_json::json!({ provider, profile }), ctx.params.clone()).await?;
        // Ok(Some(out))

        let _ = (provider, profile, ctx);
        Ok(None)
    }
}

// strategy = OAuthStrategy::new(&auth.base).with_entity_resolver(Arc::new(TypeDbUserResolver));
```

## Notes

- `dog-auth-oauth` does **not** implement an HTTP callback endpoint. Your web adapter should:
  - handle provider redirects
  - gather callback payload
  - call `OAuthService::authenticate_callback(...)` or call `AuthenticationService::create(...)` directly.
