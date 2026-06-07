# DogRS

**A modular Rust framework with multi-tenant services, hooks, and pluggable storage — built to avoid stack lock-in.**

DogRS is inspired by the simplicity of FeathersJS but reimagined for Rust.  
It provides a clean core for building flexible, multi-tenant applications where storage, transports, and execution environments can be swapped or extended without rewriting your app.

## ✨ Features (Early Outline)

- **Multi-tenant services**  
  Every request and operation runs with explicit tenant context.

- **Service hooks**  
  Before/after/around/error pipelines for validation, logging, transforms, or anything else you need.

- **Pluggable storage backends**  
  Bring your own database or use multiple ones per tenant (SQL, Mongo, TypeDB, P2P, in-memory, etc.).

- **Adapter-based architecture**  
  Use Axum today, add Warp, Actix, Serverless, or P2P transports later.

- **No stack lock-in**  
  DogRS keeps your core logic clean and portable.

- **High-Performance Lock-Free Architecture**  
  Thanks to the strict `DogAppBuilder` pattern, the entire dependency injection, hook registry, and configuration layers are completely frozen at runtime. Your hot paths run absolutely lock-free and scale linearly with zero thread-synchronization bottlenecks.

## 📦 Published Crates

All DogRS crates are now available on [crates.io](https://crates.io):

### Core Framework
- **[dog-core](https://crates.io/crates/dog-core)** `0.1.0` → Framework-agnostic core (services, hooks, tenants, storage contracts)

### Web Framework Adapters  
- **[dog-axum](https://crates.io/crates/dog-axum)** `0.1.0` → Axum adapter for HTTP APIs with multipart uploads and middleware

### Auth
- **[dog-auth](https://crates.io/crates/dog-auth)** `0.1.0` → Authentication service + strategy registry (JWT issuance)
- **[dog-auth-oauth](https://crates.io/crates/dog-auth-oauth)** `0.1.0` → Provider-agnostic OAuth strategy + orchestration

### Database Adapters
- **[dog-typedb](https://crates.io/crates/dog-typedb)** `0.1.0` → TypeDB integration with query builders and adapters

### Storage & Infrastructure
- **[dog-blob](https://crates.io/crates/dog-blob)** `0.1.0` → Production-ready blob storage with S3 compatibility and streaming

### Schema & Validation
- **[dog-schema](https://crates.io/crates/dog-schema)** `0.1.0` → Schema definition and validation utilities
- **[dog-schema-macros](https://crates.io/crates/dog-schema-macros)** `0.1.0` → Procedural macros for schema generation
- **[dog-schema-validator](https://crates.io/crates/dog-schema-validator)** `0.1.0` → Advanced validation utilities with runtime constraints

## 🚀 Quick Start

Add DogRS crates to your project:

```bash
# Core framework
cargo add dog-core

# Web development with Axum
cargo add dog-axum dog-core

# TypeDB integration
cargo add dog-typedb dog-core

# Blob storage
cargo add dog-blob

# Schema validation
cargo add dog-schema dog-schema-macros dog-schema-validator
```

## 📚 Docs

- [Configuration](docs/configuration.md)

## 🧪 Examples

- `dog-examples/auth-demo` includes an end-to-end OAuth2 Google login flow.
  - Uses `dog-auth-oauth` (enable `oauth2-client` feature for the reusable `oauth2` client helper)
  - Exposes OAuth endpoints via `dog-axum`

## 🚧 Status

DogRS is in active development.  
The goal is to build a simple but powerful foundation for real-world Rust applications without forcing a fixed stack.

---

<div align="center">

**Made by [Jitpomi](https://github.com/Jitpomi)**

</div>

Inspiration from: [FeathersJS](https://feathersjs.com/) and [NestJS](https://nestjs.com/).