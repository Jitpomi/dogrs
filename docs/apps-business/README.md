# Jitpomi Business Platform (`apps/business`)

A high-performance, multi-tenant business operating system built on the **DogRS** framework and seated directly on **TypeDB 3.x**.

---

## 1. Executive Summary

`apps/business` is designed to power core enterprise workflows:
- **Multi-tenant Organization & Team Hierarchy**: Complex subsidiaries, departments, matrix reporting, and dynamic role-based access control (RBAC).
- **Knowledge-Driven Reasoning**: Automatic permission and relationship deduction using TypeDB inference rules (e.g., inheriting permissions across parent/child organizations).
- **Multi-Protocol Service Dispatch**: High-throughput HTTP/REST APIs, real-time WebSocket feeds for live dashboard events, and administrative CLI tools.
- **Asynchronous Task & Workflow Engine**: Durable background processing for billing cycles, webhook dispatches, and multi-stage approval state machines via `dog-queue`.
- **Tenant-Scoped Asset Pipeline**: Streaming invoice and contract document storage backed by `dog-blob` and S3.

---

## 2. Technology Stack

| Layer | Technology | Role |
|---|---|---|
| **Core Runtime & Lifecycle** | [`dog-core`](https://crates.io/crates/dog-core) `0.2.0` | Application composition, lifecycle hooks, and explicit `TenantContext` routing. |
| **Primary Knowledge Database** | [`dog-typedb`](https://crates.io/crates/dog-typedb) `0.2.0` + TypeDB 3.x | Graph ontology, entities, hyper-relations, and deductive reasoning rules. |
| **Transports & Realtime** | [`dog-transport`](https://crates.io/crates/dog-transport) `0.2.0` | HTTP/Tower REST routes, WebSockets for live notifications, and CLI management commands. |
| **Identity & Authentication** | [`dog-auth`](https://crates.io/crates/dog-auth), [`dog-auth-local`](https://crates.io/crates/dog-auth-local), [`dog-auth-oauth`](https://crates.io/crates/dog-auth-oauth) `0.2.0` | Multi-tenant JWT tokens (AWS-LC crypto), Bcrypt password hashing, and OAuth2/SSO. |
| **Durable Queues & Schedulers** | [`dog-queue`](https://crates.io/crates/dog-queue) `0.2.0` | PostgreSQL/Redis background job execution, retries, DLQs, and Cron scheduling. |
| **Document & Media Storage** | [`dog-blob`](https://crates.io/crates/dog-blob) `0.2.0` | S3-compatible tenant-scoped streaming uploads and multipart attachments. |
| **Validation Engine** | [`dog-schema`](https://crates.io/crates/dog-schema), [`dog-schema-validator`](https://crates.io/crates/dog-schema-validator) `0.2.0` | Request normalization and data constraint validation. |

---

## 3. High-Level Architecture

```text
                               ┌───────────────────────────┐
                               │  Client Applications      │
                               │  (Web UI, Mobile, APIs)   │
                               └─────────────┬─────────────┘
                                             │ HTTP / WS / SSE
                                             ▼
                               ┌───────────────────────────┐
                               │       dog-transport       │
                               │  (Auth & Tenant Gateway)  │
                               └─────────────┬─────────────┘
                                             │ Verified TenantContext & Identity
                                             ▼
                               ┌───────────────────────────┐
                               │         dog-core          │
                               │    Application Engine     │
                               └──────┬──────┬──────┬──────┘
                                      │      │      │
           ┌──────────────────────────┘      │      └──────────────────────────┐
           ▼                                 ▼                                 ▼
┌───────────────────────┐         ┌───────────────────────┐         ┌───────────────────────┐
│  Domain Services      │         │   Background Engine   │         │    Blob Pipeline      │
│  (src/services/)      │         │      (dog-queue)      │         │      (dog-blob)       │
├───────────────────────┤         ├───────────────────────┤         ├───────────────────────┤
│ • iam                 │         │ • Invoice generation  │         │ • Signed contracts    │
│ • organizations       │         │ • Webhook dispatches  │         │ • Customer invoices   │
│ • billing             │         │ • Subscription cron   │         │ • Upload attachments  │
│ • documents           │         │ • Email / SMS alerts  │         └───────────────────────┘
│ • workflows           │         └───────────────────────┘
└──────────┬────────────┘
           │ TypeQL Transactions & Inferred Queries
           ▼
┌────────────────────────────────────────────────────────┐
│             TypeDB 3.x Knowledge Graph                 │
│      (Polymorphic Entities, Hyper-Relations, Rules)    │
└────────────────────────────────────────────────────────┘
```

---

## 4. Documentation Sitemap

- [**TypeDB Ontology & Schema Specification**](typedb-ontology.md): Entity hierarchy, hyper-relations, and inference rules.
- [**Raw TypeQL Schema (`schema.tql`)**](schema.tql): Ready-to-deploy schema file for TypeDB 3.x.
- [**Domain Services Design**](services.md): Detailed module specifications adhering to `docs/application-structure.md`.
- [**Transports & API Contracts**](transports-and-api.md): REST endpoints, WebSocket channel specifications, and CLI commands.
- [**Async Pipelines & Queues**](async-pipelines.md): Background job types, payload formats, and storage flows.
