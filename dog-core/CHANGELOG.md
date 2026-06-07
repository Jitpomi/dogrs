# Changelog

All notable changes to the `dog-core` crate will be documented in this file.

## [0.1.8] — Schema Eviction (Breaking)

### Breaking Changes
- **Removed:** All schema validation logic (`ValidateData`, `ResolveData`, `HookMeta`,
  `SchemaHooksExt`, `Rules`, `WriteMethods`) has been removed from `dog-core`.
- **Migrated To:** `dog-schema` crate — available at `dog_schema::schema_hooks::*`
  and re-exported at the `dog-schema` crate root (`dog_schema::SchemaHooksExt` etc).

### Why
1. **Separation of Concerns:** `dog-core` handles the DI registry and hook pipeline only.
   It should not know what a "schema" or "validation" is.
2. **Reduced footprint:** Projects using only raw `dog-core` are no longer burdened by
   validation abstractions they don't need.
3. **Cohesion:** All schema logic — macros, validation, pipeline hooks — lives in `dog-schema`.

### Who Is Affected?

**Using `#[schema]` and `dog-schema` is already in your `Cargo.toml`:**
Not affected. The macro regenerates code with the new paths automatically.

**Using `#[schema]` but `dog-schema` is NOT in your `Cargo.toml`:**
You will get an unresolved import error on the generated code. Add the dependency:
```toml
dog-schema = "0.1.8"
```

**Writing manual schema hooks (`use dog_core::schema::...`):**
Two steps required:

Step 1 — add `dog-schema` to `Cargo.toml`:
```toml
dog-schema = "0.1.8"
```

Step 2 — update `use` statements:
```rust
// Before
use dog_core::schema::{SchemaHooksExt, HookMeta, ValidateData};

// After
use dog_schema::{SchemaHooksExt, HookMeta, ValidateData};
```

### Zero capability or performance loss.
The executed code is identical — only its location changed.
