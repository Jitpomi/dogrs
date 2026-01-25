# dog-auth-local

Local (username/password) authentication strategy for DogRS, modeled after FeathersJS `@feathersjs/authentication-local`.

## What you get

- **`LocalStrategy<P>`**
  - Validates a login request (e.g. email + password)
  - Loads an entity from a configured service
  - Verifies a bcrypt password hash
  - Returns an `AuthenticationResult` with the attached entity
- **Hooks**
  - `HashPasswordHook`: hashes password fields in create/patch requests
  - `ProtectHook`: strips sensitive fields (e.g. password) from external responses

## Install

```toml
[dependencies]
dog-auth = { path = "../dog-auth" }
dog-auth-local = { path = "../dog-auth-local" }
```

## Strategy usage

`LocalStrategy` reads entity configuration from `dog-auth`’s `AuthOptions`:

- `authentication.service` (e.g. `"users"`) is required
- `authentication.entity` controls where the entity is attached in the result (defaults to `"user"`)

Minimal registration:

```rust
use std::sync::Arc;
use dog_auth::AuthenticationService;
use dog_auth_local::LocalStrategy;

fn register_local<P: Send + Clone + 'static>(auth: Arc<AuthenticationService<P>>) {
    let strategy = LocalStrategy::new(&auth.base);
    auth.register_strategy("local", Arc::new(strategy));
}
```

In many applications, you will also want to reuse the same `LocalStrategy` instance for hooks:

- `HashPasswordHook::new("password", strategy)` uses `LocalStrategy::hash_password`, so it should
  be constructed with the same options you use for authentication.

### Custom backends (TypeDB) via `LocalEntityResolver`

If your `users` service does not implement `find` (e.g. TypeDB services using custom methods),
register a resolver and `LocalStrategy` will use it instead of `authentication.service`.

```rust
use std::sync::Arc;
use dog_auth_local::{LocalEntityResolver, LocalStrategy};
use dog_core::HookContext;
use serde_json::Value;

struct TypeDbUserResolver;

#[async_trait::async_trait]
impl<P> LocalEntityResolver<P> for TypeDbUserResolver
where
    P: Send + Clone + 'static,
{
    async fn resolve_entity(
        &self,
        username: &str,
        ctx: &mut HookContext<Value, P>,
    ) -> anyhow::Result<Option<Value>> {
        // Example: call a custom method on your users service.
        // let users = ctx.services.service::<Value, P>("users")?;
        // let out = users.custom(&ctx.tenant, "findByEmail", serde_json::json!({ "email": username }), ctx.params.clone()).await?;
        // Ok(Some(out))

        let _ = (username, ctx);
        Ok(None)
    }
}

// let strategy = LocalStrategy::new(&auth.base).with_entity_resolver(Arc::new(TypeDbUserResolver));
```

### Query-capable backends via `LocalEntityQueryBuilder`

If your `users` service supports efficient filtering/limits through its params type (e.g. a Mongo adapter
that reads `params.query`), you can provide a query builder. `LocalStrategy` will call `find()` with
these modified params, and then still verify the username match as a safety check.

```rust
use std::sync::Arc;
use dog_auth_local::{LocalEntityQueryBuilder, LocalStrategy};

struct MyQueryBuilder;

impl<P> LocalEntityQueryBuilder<P> for MyQueryBuilder
where
    P: Send + Clone + 'static,
{
    fn build_find_params(&self, base: &P, _username_field: &str, _username: &str) -> P {
        // Return a clone of your params with an injected query + limit.
        // The exact mechanics depend on your params type.
        base.clone()
    }
}

// let strategy = LocalStrategy::new(&auth.base).with_entity_query_builder(Arc::new(MyQueryBuilder));
```

Authentication request shape (JSON):

```json
{
  "strategy": "local",
  "email": "me@example.com",
  "password": "secret"
}
```

If your app uses `username` instead of `email`, configure `LocalStrategyOptions` accordingly.

## Hooks

### HashPasswordHook

Use this before create/patch to hash user passwords.

- Supports dotted paths and arrays
- Uses `LocalStrategy::hash_password`

### ProtectHook

Use this after hooks to remove sensitive fields from results.

- Works for `HookResult::One` and `HookResult::Many`
- Also supports paginated-ish results with an object containing a `data` array

## Notes

- If no `LocalEntityResolver` is configured, the default entity lookup uses `service.find()` and filters in-memory.
  - This is a compatibility fallback that keeps the strategy backend-agnostic, but it can be inefficient for large datasets and it does not work for custom-only backends.
  - Prefer `LocalEntityResolver` when your backend uses custom methods (e.g. TypeDB) or when you want an efficient lookup.
