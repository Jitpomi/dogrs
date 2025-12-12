# Configuration

DogRS takes a unique configuration approach:
Instead of locking you into a specific framework (Axum, Actix, Poem), runtime (serverless, containers, P2P), or source (env vars, .toml files, cloud secrets), DogRS provides a minimal, generic configuration layer that your application can extend however it likes.
DogRS applications always own their configuration logic — DogRS simply provides a durable, structured place to store it.
This approach mirrors Feathers’ `app.set()` / `app.get()` pattern, but adapted for Rust and multi-tenant deployments.

### 🧱 DogRS Config Basics

```rust
let mut app = DogApp::<MyRecord, MyParams>::new();

app.set("paginate.default", "10");
app.set("paginate.max", "50");

assert_eq!(app.get("paginate.default"), Some("10"));
```

DogRS configuration is a simple string-based key/value store.
It does not enforce schemas, formats, or loaders.
This makes DogRS suitable for:

- CLI tools
- microservices
- monoliths
- distributed P2P nodes
- embedded edge runtimes
- WASI or serverless workers
- …all without depending on TOML/JSON/YAML parsers.

### 🌍 Environment Variable Overrides

DogRS core is intentionally environment-agnostic.
Your application chooses if it wants to support:

- .env files
- Kubernetes envFrom:
- Fly.io secrets
- Railway variables
- AWS parameter store
- P2P-distributed config nodes

Here’s a recommended helper that mirrors Feathers-style override layering:

```rust
pub fn load_env_config<R, P>(app: &mut DogApp<R, P>, prefix: &str)
where
    R: Send + 'static,
    P: Send + 'static,
{
    for (key, value) in std::env::vars() {
        // Example: ADSDOG__PAGINATE__DEFAULT => paginate.default
        if let Some(stripped) = key.strip_prefix(prefix) {
            let normalized = stripped
                .to_lowercase()
                .replace("__", ".");

            app.set(normalized, value);
        }
    }
}
```

### 🏗 Putting It Together: Defaults + Env Overrides

```rust
let mut app: DogApp<YourRecord, YourParams> = DogApp::new();

// defaults
app.set("paginate.default", "10");
app.set("paginate.max", "50");

// override using environmental prefix ADSDOG__
load_env_config(&mut app, "ADSDOG__");
```

If the environment contains:

```bash
ADSDOG__PAGINATE__DEFAULT=25
```

Then the final config values become:

```text
paginate.default = 25    // from env
paginate.max = 50        // unchanged
```

This is the exact Feathers configuration pattern:

1. Start with defaults.
2. Apply environment-specific config.
3. Apply env var overrides.

DogRS simply encourages you to implement the layering yourself so you keep total control.

### 👥 Multi-Tenant Configuration

DogRS is multi-tenant by default.
Most apps will merge config dynamically per tenant:

```rust
let tenant_id = ctx.tenant_id.as_str();
let tenant_file = format!("config/tenants/{tenant_id}.json");

let tenant_cfg = load_json_file(&tenant_file)?; // your code, not DogRS

app.set(format!("tenant.{tenant_id}.limit"), tenant_cfg.limit);
```

Config can then be accessed inside any service:

```rust
let limit = ctx
    .app
    .get(&format!("tenant.{}.limit", ctx.tenant_id))
    .unwrap_or("100");
```

### 🔌 Pluggable Config Sources

Because DogRS treats configuration as a simple mutable key/value store, apps can load configuration from absolutely any system:

- TOML (config.rs)
- JSON (settings.json)
- Environment variables
- Consul / Vault
- Local SQLite
- Remote Postgres
- Multi-tenant per-user stores
- P2P-distributed settings (GunDB, Iroh nodes, CRDTs)

Nothing in DogRS prevents or constrains this.

### 🧭 Why DogRS Does It This Way

- ✔ Zero stack lock-in  
  No TOML rules. No dependency on serde_json. No dotenv tied to a web server.
- ✔ Works in every environment  
  Cloud, edge, serverless, WASI, P2P.
- ✔ Matches the Feathers mental model  
  `app.set("x", "y")` / `app.get("x")`
- ✔ Let apps decide how to load configuration  
  DogRS stays thin; the application stays in control.
- ✔ Multi-tenant aware  
  Config works identically per tenant or globally.

### 🐶 Summary

DogRS configuration:

- Is simple (string key/value)
- Is extensible
- Has Feathers-style `.set()` / `.get()`
- Supports layered overrides
- Fits every deployment model
- Avoids coupling to any configuration file format or loader
