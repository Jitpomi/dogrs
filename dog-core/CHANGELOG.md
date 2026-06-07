# Changelog

All notable changes to the `dog-core` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.8] — 2026-06-07

> **Also in this release:** All ecosystem crates upgraded to latest dependency versions.
> See individual changelogs: [`dog-typedb`](../dog-typedb/CHANGELOG.md),
> [`dog-auth-oauth`](../dog-auth-oauth/CHANGELOG.md),
> [`dog-schema-macros`](../dog-schema-macros/CHANGELOG.md).

### Breaking Changes
- **Removed:** All schema validation logic (`ValidateData`, `ResolveData`, `HookMeta`,
  `SchemaHooksExt`, `Rules`, `WriteMethods`) has been removed from `dog-core`.
- **Migrated To:** `dog-schema` crate — available at `dog_schema::schema_hooks::*`
  and re-exported at the `dog-schema` crate root (`dog_schema::SchemaHooksExt` etc).

### Added
- `DogError` now implements `serde::Serialize` — single source of truth for the wire field layout
  (`name`, `message`, `code`, `className`, optional `data`/`errors`; `source` never emitted).
- `DogValue` enum for format-agnostic `data`/`errors` payloads when `json` feature is disabled:
  - `Null`, `Bool(bool)`, `Integer(i64)`, `Float(f64)`, `String(String)`, `Array`, `Object`
  - `Integer` and `Float` are separate variants — `i64` precision for IDs/timestamps, no silent `f64` corruption
  - `Object` uses `BTreeMap<String, DogValue>` — deterministic alphabetical field order, stable snapshots
  - **Serialization**: works for all formats including non-self-describing (Bincode, MessagePack)
  - **Deserialization**: works for self-describing formats — JSON ✅, TOML ✅, YAML ✅, MessagePack ✅.
    Bincode ❌ by design: Bincode has no type metadata in wire bytes, so `deserialize_any` is unsupported.
    The `#[serde(untagged)]` derive uses `deserialize_any` internally; variant ordering ensures
    JSON `42` → `Integer(42)`, JSON `42.0` → `Float(42.0)` with no ambiguity.
  - `DogValue::float(v: f64) -> Option<Self>` validated constructor — rejects NaN/Infinity at construction time
  - `From` impls: `String`, `&str`, `bool`, `i64`, `i32`, `u32`, `Vec<DogValue>`, `BTreeMap<String, DogValue>`
- `bail_dog!` macro extended with `errors =`, `data =`, and `data + errors` arms for inline validation error raising
- `normalize()` now correctly chain-walks `.context()` wrappers using `chain().find_map()` — a `DogError`
  buried under anyhow context is preserved (kind + message + data + errors) instead of silently becoming `GeneralError`
- `DogError` source accessors (replaces removed `pub source` field):
  - `source_ref() -> Option<&AnyError>` — borrow for logging
  - `into_source(self) -> Option<AnyError>` — consume to re-raise original cause
  - `has_source() -> bool` — probe without extracting
- `bail_dog!` supports both orderings for combined metadata: `data = d, errors = e` AND `errors = e, data = d`
- `ErrorValue` and `DogValue` re-exported at crate root: `use dog_core::ErrorValue` / `use dog_core::DogValue`
- Tests: 14 unit tests under `json` feature; 18 unit tests under `serde-only` feature; 5 under no-features:
  - `bail_dog!` macro: all 6 arms covered including reverse-order arm
  - `normalize()`: regression test for data+errors preservation through `.context()` wrappers
  - `DogValue`: round-trips, Integer/Float disambiguation, large-integer precision, nested payloads

- Separated `serde_json` from the base `serde` feature into a new `json` feature (enabled by default).
  Existing code continues to work without changes. `default-features = false` now truly removes `serde_json`.
- `dog-axum` and `dog-schema`: changed `features = ["serde"]` → `["json"]` — both use `to_json()` /
  `serde_json::Value` which require the `json` feature, not just `serde`. Previous config worked by Cargo
  feature unification accident and would silently break with `default-features = false` downstream.
- `ErrorKind`: removed `cfg_attr(serde, derive(Serialize, Deserialize))` — `DogError::serialize` never
  calls through `ErrorKind::serialize`; the derive was dead code.
- `ErrorKind` is now `#[non_exhaustive]` — new variants (e.g. `PaymentRequired`) may be added in
  future minor releases without breaking downstream code. External `match` arms must include `_`.
- `DogError` builder methods (`with_data`, `with_errors`, `with_source`) and all conversion/query
  methods (`into_anyhow`, `from_anyhow`, `normalize`, `sanitize_for_client`) are `#[must_use]`.
  The compiler warns if the return value is silently discarded.

### Fixed
- `source` field (`AnyError`) is never emitted in serialized output — internal stack traces and error
  details cannot leak to clients through `to_json()` or any serde serializer.
- Serialized field `class_name` → `className` (camelCase) to match the FeathersJS client contract.
- `normalize()` previously used `downcast::<DogError>()` which only checked the root type — a
  `DogError` wrapped in `.context()` was silently demoted to `GeneralError`. Fixed.
- **`normalize()` slow path silently dropped `data` and `errors`** when the error was wrapped in
  `.context()`. Fixed: all four user-visible fields (kind, message, data, errors) are preserved.
- **`bail_dog!` missing `errors = e, data = d` arm** — reverse ordering fell through to the
  format string arm producing a confusing `format!` compile error. Symmetric arm added.
- Examples (`blog-axum`, `social-typedb`, `auth-demo`): changed `dog-core` feature from
  `["serde"]` → `["json"]` — all three use `serde_json::Value` and were silently relying on
  Cargo feature unification via `dog-axum`.
- `dog-axum`: updated stale `dog-core` version constraint `0.1.5` → `0.1.8`.
- `bail_dog!` metadata tests guarded with `#[cfg(any(feature = "serde", feature = "json"))]` —
  prevents false failures under `--no-default-features`.

### Security
- **`DogError::source` made private** — the invariant (source never serialized, safe for clients)
  was documented but not enforced. `pub source` allowed any downstream crate to read internal
  error details directly or construct `DogError` via struct literal with a custom `source`,
  bypassing `sanitize_for_client()`. Field is now `source: Option<AnyError>` (private).
  Access via `source_ref()`, `into_source()`, `has_source()`, or `with_source()` builder.

### Why (Schema Eviction)
1. **Separation of Concerns:** `dog-core` handles the DI registry and hook pipeline only.
   It should not know what a "schema" or "validation" is.
2. **Reduced footprint:** Projects using only raw `dog-core` are no longer burdened by
   validation abstractions they don't need.
3. **Cohesion:** All schema logic — macros, validation, pipeline hooks — lives in `dog-schema`.

### Migration (Schema Eviction)

**Using `#[schema]` and `dog-schema` is already in your `Cargo.toml`:**
Not affected. The macro regenerates code with the new paths automatically.

**Using `#[schema]` but `dog-schema` is NOT in your `Cargo.toml`:**
You will get an unresolved import error on the generated code. Add the dependency:
```toml
dog-schema = "0.1.8"
```

**Writing manual schema hooks (`use dog_core::schema::...`):**

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
