# dog-schema

Validation and normalization hooks for DogRS services. This crate is independent
of the HTTP server, transport and database. It exports the `#[schema]` module
attribute, `SchemaHooksExt`, `Rules`, and structured `SchemaErrors`.

It does **not** generate JSON Schema documents or provide a `Schema` derive,
`json_schema()`, or `from_json()` API. Earlier README examples claiming those
APIs were incorrect. For JSON Schema generation, use a separate library in your
application. The implementation described here is the repository's 0.2.0 API;
check the published version before selecting a registry dependency.

## A service schema

The macro generates public `resolve_create`, `validate_create`, `register`, and,
when a patch struct exists, `resolve_patch` and `validate_patch` functions.
Consumers need `dog-schema`, `dog-core`, `serde_json`, and `anyhow` dependencies.

```rust
use dog_schema::schema;

#[schema(service = "posts")]
mod posts {
    #[create]
    pub struct CreatePost {
        #[dog(trim, min_len(3), max_len(100))]
        pub title: String,
        pub tags: Vec<String>,
        #[dog(default = false)]
        pub published: bool,
    }

    #[patch]
    pub struct PatchPost {
        #[dog(trim, min_len(3), max_len(100))]
        pub title: Option<String>,
        pub published: Option<bool>,
    }
}

let mut builder = dog_core::DogApp::<serde_json::Value, ()>::builder();
// Register your service implementation under "posts" as well.
posts::register(&mut builder)?;
let app = builder.build();
# Ok::<(), anyhow::Error>(())
```

Registration normalizes then validates create and update against the create
schema. PATCH uses only the patch schema, with its own normalization and checks.
Read and remove methods are unaffected. A missing patch schema rejects PATCH;
it never silently permits an unvalidated partial write. Call `register` once per
service. When invoking the generated functions directly, call the resolver before
the validator. Validation functions do not mutate data.

## Built-in validation contract

- Every provided field must deserialize into its declared Rust type. Numeric
  ranges, collection elements and nested object shapes follow Serde. Custom field
  types must implement owned deserialization; nested business rules need custom
  hooks or the optional validator backend.
- Missing create/update fields are rejected unless they are `Option<T>` or marked
  `#[dog(optional)]`. Boolean defaults are inserted by the create/update resolver.
- PATCH permits omitted fields. Explicit `null` must be accepted by the declared
  type: for example, `Option<String>` accepts it and `String` does not. Null stays
  in the payload; the service decides whether it means clearing a value.
- Unknown fields are rejected. Validation is not authorization: services must
  still enforce tenant ownership, permissions, relationship existence and other
  business invariants.
- Strings must contain a non-whitespace character. `trim` normalizes a string;
  `min_len(n)` and `max_len(n)` count Unicode scalar values, not bytes or grapheme
  clusters, in the value presented to validation.
- Supported field rules are `trim`, `optional`, `min_len(n)`, `max_len(n)`, and
  boolean `default = true/false`. PATCH does not insert defaults.
- Unknown or duplicated rules, malformed values, conflicting bounds, unsupported
  rule/type combinations, duplicate schema markers, tuple structs, conditional fields and generic
  schema structs fail at compile time. Rules such as `relation` were previously
  ignored and are now rejected.
- Built-in schema structs use their Rust field names (including raw identifiers).
  Serde rename/flatten/custom field attributes require the validator backend;
  they are rejected here rather than silently interpreted differently.

For example, a typo must fail compilation:

```compile_fail
#[dog_schema::schema(service = "posts")]
mod posts {
    #[create]
    struct Create { #[dog(min_lne(3))] title: String }
}
```

An unknown backend must also fail:

```compile_fail
#[dog_schema::schema(service = "posts", backend = "unknown")]
mod posts { #[create] struct Create { title: String } }
```

## Custom hooks and structured errors

```rust
use dog_schema::{Rules, SchemaHooksExt};
let mut hooks = dog_core::ServiceHooks::<String, ()>::new();
hooks.schema(|s| {
    s.on_create().resolve(|data, _| { *data = data.trim().to_owned(); Ok(()) });
    s.on_create().validate(|data, _| {
        Rules::new().non_empty("name", data).max_len("name", data, 100).check()
    });
});
```

A method selector applies to the **next** resolve/validate call only. Repeat
`on_create()`, `on_patch()`, or `on_update()` for each hook. Without a selector,
a hook applies to create, patch and update. All-write hooks execute before
method-specific hooks, so do not split a dependent normalization/validation pair
between those buckets.

`Rules` trims whitespace when measuring lengths and accumulates field errors.
Call `.check()` to propagate them. It returns a `DogError` with status 422 and
field arrays in `errors`, as do macro validators and missing-data failures.
Error messages from field deserializers are not echoed to clients. Resolver and
validator closures are synchronous; implement `DogBeforeHook` directly for async
work. Keep input-size/time limits at the application or transport boundary.

## Optional validator backend

`#[schema(service = "users", backend = "validator")]` delegates to
`dog-schema-validator`. Its create/patch structs must implement
`serde::Deserialize` and `validator::Validate`. Use Serde and `#[validate(...)]`
attributes, not built-in `#[dog(...)]` rules. See that crate's README for an
executable example. It respects Serde field names and unknown-field policy;
use `#[serde(deny_unknown_fields)]` when strict input is required.

## Migration notes

This hardening rejects inputs previously accepted accidentally: wrong types,
unknown fields, unsupported declarations, and nulls for non-nullable patch fields.
Create/update values are now normalized before validation; patch strings marked
`trim` are normalized too. `Rules` errors are structured 422 responses instead of
generic internal errors. Review applications relying on the older permissive
behavior before upgrading. No schema generation or database relationship checks
are added by this change.
