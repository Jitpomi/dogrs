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

## 📦 Crates (Workspace)

dog-core → Framework-agnostic core (services, hooks, tenants, storage contracts)
dog-axum → Axum adapter for HTTP APIs


More adapters coming soon.

## 🚧 Status

- DogRS is in active development.  
- The goal is to build a simple but powerful foundation for real-world Rust applications without forcing a fixed stack.

