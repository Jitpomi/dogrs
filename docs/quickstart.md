# DogRS Quickstart Guide

Why use a modular framework like DogRS? 

Consider an E-Commerce Platform. You often have two drastically different requirements:
1. **Shopping Carts (High Frequency):** Users constantly adding and removing items. You want this data in a blazing fast, distributed memory cache like **Redis**.
2. **Product Recommendations (High Complexity):** Finding products bought by users with similar profiles. You want this in a powerful Systems Database like **TypeDB**.

In a traditional setup, you might build two separate microservices. With DogRS, you can build two transport-agnostic services backed by completely different databases, protect them with the same authentication hooks, and expose them simultaneously on the same HTTP port.

---

## 1. Environment Setup

Before writing any code, we need to spin up our Redis and TypeDB instances. 

You can use a simple Docker command for each:

```bash
docker run --name typedb -d -v typedb-data:/opt/typedb/server/data -p 1729:1729 typedb/typedb:latest
docker run --name redis -d -p 6379:6379 redis:alpine
```

**Or, create a `docker-compose.yml` file in your project root:**

```yaml
version: '3.8'
services:
  typedb:
    image: typedb/typedb:latest
    ports:
      - "1729:1729"
    volumes:
      - typedb-data:/opt/typedb/server/data
  redis:
    image: redis:alpine
    ports:
      - "6379:6379"

volumes:
  typedb-data:
```
Then run `docker-compose up -d`.

---

## 2. Project Setup

Create a new Rust project and add the core dependencies.

```bash
cargo new dog-ecommerce
cd dog-ecommerce

# Add DogRS crates
cargo add dog-core dog-axum dog-auth dog-typedb

# Add the official database drivers
cargo add typedb-driver redis

# Add async runtime and utilities
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json
cargo add anyhow async-trait
```

---

## 3. The TypeDB Schema

TypeDB is a strongly-typed database. Before we can insert or query recommendations, we must define what a "user", "product", and "purchase" is.

Create a file called `schema.tql` in your `src` directory:

```typeql
define

# Entities
user sub entity,
    owns user-id @key;

product sub entity,
    owns product-id @key;

# Relations
purchase sub relation,
    relates buyer,
    relates bought-item;

user plays purchase:buyer;
product plays purchase:bought-item;

# Attributes
user-id sub attribute, value string;
product-id sub attribute, value string;
```

---

## 4. The Database Adapters

Let's create two database states: a Redis connection for shopping carts, and a TypeDB connection for product recommendations.

Open `src/main.rs`:

```rust
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use redis::AsyncCommands; // Brings in Redis async methods

use dog_core::{DogService, TenantContext};
use dog_core::hooks::{DogBeforeHook, HookContext};
use dog_core::app::DogAppBuilder;

use dog_typedb::{TypeDBAdapter, load_schema_from_file};
use typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TypeDBDriver};

// 1A. Redis Database State (Shopping Carts)
pub struct CartRedisState {
    pub client: redis::Client,
}

// 1B. TypeDB Database State (Recommendations)
pub struct RetailTypeDBState {
    pub driver: Arc<TypeDBDriver>,
    pub database: String,
}
impl dog_typedb::adapter::TypeDBState for RetailTypeDBState {
    fn driver(&self) -> &Arc<TypeDBDriver> { &self.driver }
    fn database(&self) -> &str { &self.database }
}
```

---

## 5. The Services

We encapsulate our business logic into two transport-agnostic services. Notice how DogRS allows us to talk to Redis and TypeDB side-by-side using the same interface.

```rust
// 2A. Shopping Cart Service
pub struct CartService {
    state: Arc<CartRedisState>,
}

#[async_trait]
impl DogService<Value, ()> for CartService {
    async fn find(&self, ctx: &TenantContext, _params: ()) -> Result<Vec<Value>> {
        let mut conn = self.state.client.get_async_connection().await?;
        let key = format!("cart:{}", ctx.tenant_id);
        
        let items: Vec<String> = conn.lrange(key, 0, -1).await?;
        let parsed = items.into_iter().filter_map(|s| serde_json::from_str(&s).ok()).collect();
        Ok(parsed)
    }
    
    async fn create(&self, ctx: &TenantContext, data: Value, _params: ()) -> Result<Value> {
        let mut conn = self.state.client.get_async_connection().await?;
        let key = format!("cart:{}", ctx.tenant_id);
        
        let mut item = data.clone();
        item["tenant_id"] = json!(ctx.tenant_id);
        item["added_at"] = json!(chrono::Utc::now().to_rfc3339());
        
        let item_str = serde_json::to_string(&item)?;
        let _: () = conn.rpush(key, item_str).await?;
        Ok(item)
    }
    
    async fn get(&self, _c: &TenantContext, _id: &str, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn update(&self, _c: &TenantContext, _id: &str, _d: Value, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn patch(&self, _c: &TenantContext, _id: &str, _d: Value, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn remove(&self, _c: &TenantContext, _id: &str, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn custom(&self, _c: &TenantContext, _m: &str, _d: Option<Value>, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
}

// 2B. Recommendation Service
pub struct RecommendationService {
    adapter: TypeDBAdapter,
}

#[async_trait]
impl DogService<Value, ()> for RecommendationService {
    async fn custom(&self, _ctx: &TenantContext, method: &str, data: Option<Value>, _params: ()) -> Result<Value> {
        let payload = data.ok_or_else(|| {
            dog_core::errors::DogError::new(dog_core::errors::ErrorKind::BadRequest, "Missing request body".to_string())
                .into_anyhow()
        })?;
        match method {
            "read" => self.adapter.read(payload).await,
            "write" => self.adapter.write(payload).await,
            _ => anyhow::bail!("Unknown method: {}", method),
        }
    }
    
    async fn find(&self, _c: &TenantContext, _p: ()) -> Result<Vec<Value>> { anyhow::bail!("Not implemented") }
    async fn get(&self, _c: &TenantContext, _id: &str, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn create(&self, _c: &TenantContext, _d: Value, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn update(&self, _c: &TenantContext, _id: &str, _d: Value, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn patch(&self, _c: &TenantContext, _id: &str, _d: Value, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
    async fn remove(&self, _c: &TenantContext, _id: &str, _p: ()) -> Result<Value> { anyhow::bail!("Not implemented") }
}
```

---

## 6. Auth & Hooks

Let's create an `EnforceAuth` hook to ensure only authenticated users can modify carts or query the recommendation engine.

```rust
// 3. The Hook
pub struct EnforceAuth;

#[async_trait]
impl DogBeforeHook<Value, ()> for EnforceAuth {
    async fn run(&self, ctx: &mut HookContext<Value, ()>) -> Result<()> {
        if !ctx.tenant.is_authenticated() {
            anyhow::bail!("Unauthorized: Invalid API Token");
        }
        Ok(())
    }
}
```

---

## 7. Wiring It All Together

We use `DogAppBuilder` to assemble our logic, attach our hook, and expose both services over HTTP via Axum.

```rust
// 4. Application Entrypoint
#[tokio::main]
async fn main() -> Result<()> {
    // A. Initialize Databases
    
    // Connect to Redis
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let redis_client = redis::Client::open(redis_url)?;
    let cart_state = Arc::new(CartRedisState { client: redis_client });
    
    // Connect to TypeDB
    let credentials = Credentials::new("admin", "password");
    let options = DriverOptions::new(DriverTlsConfig::disabled());
    let typedb_host = std::env::var("TYPEDB_HOST").unwrap_or_else(|_| "127.0.0.1:1729".to_string());
    let addresses = Addresses::try_from_address_str(&typedb_host)?;
    
    let driver = Arc::new(TypeDBDriver::new(addresses, credentials, options).await?);
    
    // Ensure the TypeDB Database actually exists
    let db_name = "retail-db";
    if !driver.databases().all().await?.iter().any(|db| db.name() == db_name) {
        println!("Creating TypeDB database...");
        driver.databases().create(db_name).await?;
    }
    
    let typedb_state = Arc::new(RetailTypeDBState { 
        driver: driver.clone(), 
        database: db_name.to_string() 
    });

    // Load our strictly-typed schema into the database
    let schema_paths = ["src/schema.tql"];
    load_schema_from_file(&driver, db_name, &schema_paths).await?;
    
    // B. Build the Application
    let mut builder = DogAppBuilder::<Value, ()>::new();
    
    // Secure our endpoints by attaching the hook to 'create' (cart) and 'custom' (recommendations)
    builder.hooks(|h| {
        h.before_create(Arc::new(EnforceAuth));
        h.before_custom(Arc::new(EnforceAuth));
    });

    let app = builder.build();

    // C. Instantiate Services
    let cart_svc = Arc::new(CartService { state: cart_state });
    let rec_svc = Arc::new(RecommendationService { adapter: TypeDBAdapter::new(typedb_state) });

    // D. Mount to the Axum Transport Adapter (REST API)
    let ax = dog_axum::axum(app.clone())
        .with_cors(tower_http::cors::CorsLayer::permissive())
        .use_service("/cart", Arc::clone(&cart_svc))
        .use_service("/recommendations", Arc::clone(&rec_svc));

    // E. Simultaneously access the services natively from an internal background task
    let app_clone = app.clone();
    tokio::spawn(async move {
        let ctx = TenantContext::new("internal_cron");
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            if let Ok(svc) = app_clone.service("cart") {
                let _ = svc.find(&ctx, ()).await;
            }
        }
    });

    println!("Listening on http://127.0.0.1:3000");
    ax.listen("127.0.0.1:3000".to_string()).await?;

    Ok(())
}
```

---

## 8. The Storefront Frontend

Create an `index.html` file to see how an e-commerce storefront interacts with both systems. This provides a clean, minimal UI to test the endpoints.

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Storefront API Demo</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            background-color: #f9fafb;
            color: #111827;
            line-height: 1.5;
            padding: 2rem;
            max-width: 1000px;
            margin: 0 auto;
        }
        .header {
            margin-bottom: 2rem;
            padding-bottom: 1rem;
            border-bottom: 1px solid #e5e7eb;
        }
        .header h1 {
            margin: 0 0 0.5rem 0;
        }
        .header p {
            margin: 0;
            color: #4b5563;
        }
        .grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 2rem;
        }
        .panel {
            background: #ffffff;
            border: 1px solid #e5e7eb;
            border-radius: 8px;
            padding: 1.5rem;
            box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
        }
        .panel h2 {
            margin-top: 0;
            font-size: 1.25rem;
            color: #1f2937;
        }
        .input-group {
            display: flex;
            gap: 0.5rem;
            margin-bottom: 1rem;
        }
        input[type="text"] {
            flex: 1;
            padding: 0.5rem;
            border: 1px solid #d1d5db;
            border-radius: 4px;
            font-size: 0.875rem;
        }
        button {
            background-color: #2563eb;
            color: white;
            border: none;
            padding: 0.5rem 1rem;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.875rem;
            font-weight: 500;
        }
        button:hover {
            background-color: #1d4ed8;
        }
        button:disabled {
            background-color: #9ca3af;
            cursor: not-allowed;
        }
        .list-container {
            border: 1px solid #e5e7eb;
            border-radius: 4px;
            height: 250px;
            overflow-y: auto;
            background-color: #f9fafb;
        }
        ul {
            list-style: none;
            padding: 0;
            margin: 0;
        }
        li {
            padding: 0.75rem;
            border-bottom: 1px solid #e5e7eb;
            font-size: 0.875rem;
            font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
        }
        li:last-child {
            border-bottom: none;
        }
        .error {
            color: #dc2626;
            padding: 0.75rem;
            font-size: 0.875rem;
        }
    </style>
</head>
<body>
    <div class="header">
        <h1>Storefront API Demo</h1>
        <p>A multi-tenant architecture running Redis and TypeDB concurrently.</p>
    </div>
    
    <div class="grid">
        <div class="panel">
            <h2>Shopping Cart (Redis Cache)</h2>
            <div class="input-group">
                <input type="text" id="product-id" placeholder="Product ID (e.g. PRD-123)">
                <button onclick="addToCart()">Add Item</button>
            </div>
            <div class="list-container">
                <ul id="cart-list"></ul>
            </div>
        </div>

        <div class="panel">
            <h2>Recommendations (TypeDB)</h2>
            <div class="input-group">
                <button id="rec-btn" onclick="getRecommendations()" style="width: 100%;">Run Query</button>
            </div>
            <div class="list-container">
                <ul id="rec-list"></ul>
            </div>
        </div>
    </div>

    <script>
        const headers = {
            "Content-Type": "application/json",
            "X-Tenant-Id": "store_us",
            // "Authorization": "Bearer token" // Uncomment to bypass the EnforceAuth hook
        };

        async function fetchCart() {
            try {
                const res = await fetch("http://127.0.0.1:3000/cart", { headers });
                const data = await res.json();
                const list = document.getElementById("cart-list");
                list.innerHTML = data.map(p => `<li>Added: ${p.pid}</li>`).join('');
            } catch (e) {
                document.getElementById("cart-list").innerHTML = `<div class="error">Failed to connect to API.</div>`;
            }
        }

        async function addToCart() {
            const payload = { pid: document.getElementById("product-id").value || "Unknown Item" };
            const res = await fetch("http://127.0.0.1:3000/cart", {
                method: "POST", headers, body: JSON.stringify(payload)
            });
            if (res.ok) {
                document.getElementById("product-id").value = "";
                fetchCart();
            } else {
                alert(`Error: ${await res.text()}`);
            }
        }

        async function getRecommendations() {
            const btn = document.getElementById("rec-btn");
            btn.innerText = "Querying...";
            btn.disabled = true;
            
            const query = "match $u isa user; $p1 isa product, has product-id 'PRD-123'; ($u, $p1) isa purchase; ($u, $p2) isa purchase; get $p2;";
            try {
                const res = await fetch("http://127.0.0.1:3000/recommendations/custom/read", {
                    method: "POST", headers, body: JSON.stringify({ query })
                });
                
                const list = document.getElementById("rec-list");
                if (res.ok) {
                    const data = await res.json();
                    list.innerHTML = data.map(p => `<li>${JSON.stringify(p)}</li>`).join('');
                } else {
                    alert(`Error: ${await res.text()}`);
                }
            } catch (e) {
                document.getElementById("rec-list").innerHTML = `<div class="error">Failed to connect to TypeDB route.</div>`;
            } finally {
                btn.innerText = "Run Query";
                btn.disabled = false;
            }
        }
        
        fetchCart();
    </script>
</body>
</html>
```

## Summary

In just one file, you successfully:
1. Ran a **Redis** Cache AND a **TypeDB** Systems Database simultaneously.
2. Separated business logic into distinct, transport-agnostic services (`CartService` and `RecommendationService`).
3. Protected both databases with the exact same reusable middleware (`EnforceAuth` hook).
4. Proved that services can be exposed to Axum (REST) and called natively from background workers at the exact same time.
5. Built a basic JavaScript storefront to interface with the API.

---

## 9. Containerizing Your App (Full Compose)

Ready to ship? Package your entire stack (the Rust API, Redis, and TypeDB) into a single full `docker-compose.yml`.

First, create a `Dockerfile` for your Rust app:

```dockerfile
FROM rust:1.77-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/dog-ecommerce /app/dog-ecommerce
EXPOSE 3000
CMD ["./dog-ecommerce"]
```

Now, replace your `docker-compose.yml` with this complete stack configuration:

```yaml
version: '3.8'

services:
  typedb:
    image: typedb/typedb:latest
    ports:
      - "1729:1729"
    volumes:
      - typedb-data:/opt/typedb/server/data

  redis:
    image: redis:alpine
    ports:
      - "6379:6379"

  api:
    build: .
    ports:
      - "3000:3000"
    depends_on:
      - typedb
      - redis
    environment:
      # Tell the Rust app to connect to the docker network services
      - TYPEDB_HOST=typedb:1729
      - REDIS_URL=redis://redis/

volumes:
  typedb-data:
```

Now you can bring up your entire infrastructure and API with one command:
```bash
docker-compose up --build -d
```
