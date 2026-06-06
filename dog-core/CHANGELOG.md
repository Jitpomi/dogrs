# Changelog

All notable changes to the `dog-core` crate will be documented in this file.

## [Unreleased]

### Breaking Changes (Schema Eviction)
- **Removed:** All schema validation logic (`ValidateData`, `ResolveData`, `HookMeta`, and the `SchemaHooksExt` builder) has been entirely removed from `dog-core`.
- **Migrated To:** This logic now lives exclusively inside the `dog-schema` crate (`dog_schema::hooks::*`).

### Why We Did It (The Benefits)
1. **Separation of Concerns:** `dog-core` is the foundational engine of the framework. Its only job is to handle the dependency injection registry and execute the hooks pipeline. It should not need to know what a "schema" is or how data is "validated." 
2. **Reduced Core Footprint:** By moving validation logic to the `dog-schema` crate, developers who only want to use the raw `dog-core` pipeline aren't burdened by validation abstractions they might not need.
3. **Ecosystem Cohesion:** It makes logical sense that all things schema-related (including the hooks that enforce them) live inside the `dog-schema` crate.

### Losses & Performance
- **Zero performance loss:** The code executed is exactly the same, just located in a different crate.
- **Zero capability loss:** The exact same validation features exist.
- **Temporary ergonomics loss (for manual users):** Existing developers who wrote validation code manually will temporarily experience compiler errors until they update their import paths.

### Who is Affected?
1. **Developers using the `#[schema]` macro:** **NOT AFFECTED.** The macro was updated to generate the new paths automatically. Upon running `cargo update`, everything will compile seamlessly.
2. **Developers writing manual schema hooks:** **AFFECTED.** If a developer manually imported `dog_core::schema::SchemaHooksExt` to write custom validation logic outside of the macro, their compiler will fail.

### How to Recover (Migration Steps)
If you are manually utilizing schema hooks, simply update your `use` statements:

**Old Code:**
```rust
use dog_core::schema::{SchemaHooksExt, HookMeta, ValidateData};
```

**New Code:**
```rust
use dog_schema::hooks::{SchemaHooksExt, HookMeta, ValidateData};
```

Everything else functions exactly as it did before.
