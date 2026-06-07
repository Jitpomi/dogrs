# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased] - Performance Refactoring

### 💥 Breaking Changes: The `DogAppBuilder` Pattern
We have introduced a major breaking change to the core initialization flow of DogRS. The `DogApp` structure is now immutable once built. A new `DogAppBuilder` has been introduced to handle all dependency injection and service registration.

#### 💡 What we did
- Removed `any_state` (`RwLock<HashMap>`) from the `DogApp` runtime context.
- Removed `RwLock` from the event hub and hooks registry in the hot path.
- Introduced `DogAppBuilder` to handle:
  - Configuration (`builder.config()`, `builder.set()`)
  - Hook Registration (`builder.hooks()`, `builder.service_hooks()`)
  - Service Registration (`builder.register_service()`)
- `DogApp` is now created by calling `builder.build()`. 

#### 🎯 Why we did it & Benefits
Previously, the `DogApp` state used an `RwLock` to allow services to be registered and retrieved at runtime. Under high concurrency, this created a massive thread-synchronization bottleneck, preventing the server from scaling linearly across CPU cores. 
By adopting the Builder Pattern:
1. **Lock-Free Hot Path**: We completely eliminated `RwLock` from the request lifecycle. Reading configuration and looking up services is now lock-free and heavily optimized.
2. **True Rust Idioms**: It enforces a strict initialization phase versus a runtime phase, which is much more idiomatic to Rust's type-system and concurrency model (preventing deadlocks and poisoned locks).
3. **Performance**: We anticipate a massive throughput improvement for high-traffic environments. 

#### 🗣️ Why is it worth the effort? (Why pass the Builder around?)
You might find yourself refactoring functions like `pub fn configure(app: &DogApp)` to `pub fn configure(builder: &mut DogAppBuilder)`. The reason for this strict setup phase is **immutability**. Because the final `DogApp` is completely immutable, it guarantees that no background thread or rogue service can mutate the global state while requests are flying through. This means that reading configurations and resolving services across all threads can happen in parallel without any lock contention, giving your application raw, unthrottled performance at scale.

#### 📉 Losses & Who is affected
- **Loss of Runtime Mutation**: You can no longer register new services or change hooks *after* the application has started handling requests. 
- **Affected**: Any developer upgrading to this version. 

#### 🛠️ How to recover (Migration Guide)
Migrating is straightforward. You must separate your initialization logic from your runtime logic.

**Before:**
```rust
let app = DogApp::new();
app.register_service("users", users_svc);
app.service("users")?.hooks(|h| { ... });
let ax = axum(app);
```

**After:**
```rust
let mut builder = DogAppBuilder::new();
builder.register_service("users", users_svc);
builder.service_hooks("users", |h| { ... });

let app = builder.build();
let ax = axum(app);
```

## `dog-auth` AuthenticationBuilder Migration
We completely removed all `RwLock` instances from `AuthenticationService` as well!

### What Changed?
1. Introduced `AuthenticationBuilder<P>` to build your auth service before application launch.
2. The core `AuthenticationBase` and its strategies (`JwtStrategy`, `LocalStrategy`, `OAuthStrategy`) no longer use cyclic locking (`Weak` references) to resolve the configuration.

### How to Migrate
Previously you might have written:
```rust
let auth = Arc::new(AuthenticationService::new(builder, Some(opts))?);
jwt::register_jwt(&auth);
local::register_local(Arc::clone(&auth));
```

Now, use the new `builder` method:
```rust
let mut auth_builder = AuthenticationService::builder(builder, Some(opts))?;

// Register your strategies on the builder
jwt::register_jwt(&mut auth_builder);
local::register_local(&mut auth_builder);

// Build and install
let auth = Arc::new(AuthenticationService::new(Arc::new(auth_builder.build())));
let adapter = AuthenticationService::install(builder, auth.clone());
```

---

## `dog-core` DogEventHub Optimization
The main event hub inside `dog-core` no longer uses `RwLock`. Event registration methods like `.on` and `.on_str` have been moved to `DogAppBuilder`. Wait-free event emissions (e.g. `once` listeners) are now implemented via atomic states internally instead of write-locking the array.

### How to Migrate
Previously you might have registered events after application launch:
```rust
app.on_str("messages.created", Arc::new(|data, ctx| { ... }));
```

Now, configure your listeners during application boot on the `DogAppBuilder`:
```rust
builder.on_str("messages.created", Arc::new(|data, ctx| { ... }));
```

---

## 🛠️ Additional Optimizations
For schema validation macros, ensure your `register` function accepts `&mut DogAppBuilder` instead of `&DogApp`.
