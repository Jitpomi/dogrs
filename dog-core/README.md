






# dog-core

[![Crates.io](https://img.shields.io/crates/v/dog-core.svg)](https://crates.io/crates/dog-core)
[![Documentation](https://docs.rs/dog-core/badge.svg)](https://docs.rs/dog-core)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

**Core traits and utilities for the DogRS ecosystem - a modular Rust framework for building scalable applications**

dog-core provides the foundational abstractions that power the DogRS framework: services, hooks, tenant contexts, and storage contracts. It's designed to keep your core logic clean and portable across different adapters and environments.

## Features

- **Framework-agnostic core** - No coupling to specific web frameworks or databases
- **Multi-tenant services** - Built-in tenant context for SaaS applications
- **Service hooks** - Before/after/around/error pipelines for cross-cutting concerns
- **Storage contracts** - Pluggable storage backends without vendor lock-in
- **Async-first design** - Built for modern async Rust applications

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-core = "0.1.0"
```

### Basic Service Example

```rust
use dog_core::{DogService, TenantContext, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateUserRequest {
    name: String,
    email: String,
}

#[derive(Serialize)]
struct User {
    id: u32,
    name: String,
    email: String,
}

struct UserService;

#[async_trait]
impl DogService<CreateUserRequest, ()> for UserService {
    type Output = User;
    
    async fn create(&self, tenant: TenantContext, data: CreateUserRequest) -> Result<User> {
        // Your business logic here
        Ok(User {
            id: 1,
            name: data.name,
            email: data.email,
        })
    }
}
```

## Core Concepts

### Services
Services implement your business logic through the `DogService` trait:

```rust
#[async_trait]
pub trait DogService<TData, TQuery> {
    type Output;
    
    async fn create(&self, tenant: TenantContext, data: TData) -> Result<Self::Output>;
    async fn read(&self, tenant: TenantContext, query: TQuery) -> Result<Self::Output>;
    async fn update(&self, tenant: TenantContext, data: TData) -> Result<Self::Output>;
    async fn delete(&self, tenant: TenantContext, query: TQuery) -> Result<()>;
}
```

### Tenant Context
Multi-tenant applications get built-in tenant isolation:

```rust
let tenant = TenantContext::new("tenant-123")
    .with_actor("user-456")
    .with_trace_id("req-789");

let result = service.create(tenant, request_data).await?;
```

### Storage Adapters
Pluggable storage without vendor lock-in:

```rust
use dog_core::{StorageAdapter, StorageResult};

#[async_trait]
impl StorageAdapter for MyDatabase {
    async fn get(&self, tenant: &TenantContext, key: &str) -> StorageResult<Option<Vec<u8>>>;
    async fn put(&self, tenant: &TenantContext, key: &str, value: Vec<u8>) -> StorageResult<()>;
    async fn delete(&self, tenant: &TenantContext, key: &str) -> StorageResult<()>;
}
```

## Architecture

dog-core follows a clean separation of concerns:

```
┌─────────────────┐
│   Your App      │  ← Business logic
├─────────────────┤
│   dog-axum      │  ← HTTP adapter
│   dog-typedb    │  ← Database adapter  
│   dog-blob      │  ← Storage adapter
├─────────────────┤
│   dog-core      │  ← Core abstractions
└─────────────────┘
```

## Ecosystem

dog-core works with these adapters:

- **[dog-axum](https://crates.io/crates/dog-axum)** - Axum web framework integration
- **[dog-typedb](https://crates.io/crates/dog-typedb)** - TypeDB database adapter
- **[dog-blob](https://crates.io/crates/dog-blob)** - Blob storage infrastructure
- **[dog-schema](https://crates.io/crates/dog-schema)** - Schema validation utilities

## Examples

See the `dog-examples/` directory for complete applications:

- **music-blobs** - Media streaming service
- **blog-axum** - REST API with CRUD operations  
- **social-typedb** - Social network with TypeDB
- **fleet-queue** - Fleet management with background jobs

## License

MIT OR Apache-2.0

---

<div align="center">

**Made by [Jitpomi](https://github.com/Jitpomi)**

</div>
