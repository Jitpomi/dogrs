# Changelog

All notable changes to the `dog-typedb` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.4] — 2026-10-20

- Return a structured 403 when schema metadata does not grant an operation,
  including whole-object deletion. Preserve deny-by-default and rollback; never
  infer permission or HTTP status from query literals or error-message text.
- Keep malformed declared policies and other internal failures as server errors,
  and retain private diagnostic causes without exposing schema details to clients.
- Add live TypeDB and consuming-server HTTP regressions proving denied deletes
  leave persisted objects unchanged, with and without a final fetch.

## [0.3.3] — 2026-10-20

- Reuse compiler-generated schema introspection within a transaction and across
  mutation checkpoints. Do not cache frontend queries, authorization decisions,
  membership or data, and never reuse metadata across transactions. Fix excessive
  round trips that exhausted the policy deadline during concurrent grant reviews;
  add real compiler round-trip and transaction-boundary regressions.
- Preserve private schema-policy failure causes and execution phases instead of
  discarding them behind an opaque 500. Keep causes out of client responses;
  distinguish a pre-commit deadline from an unknown commit outcome. Add regression
  coverage for cause retention, deadline classification and client redaction.
- Return 409 for a server-confirmed STC2 isolation-conflict rejection. Keep commit
  deadlines and transport failures as unknown outcomes and never replay writes.

## [0.3.2] — 2026-10-20

- Add an experimental `isolation::SchemaPolicyAdapter` that accepts frontend
  TypeQL without registering query templates. It uses the official TypeQL parser,
  schema metadata, mandatory TypeDB policy functions and server-owned typed inputs.
- Extend structural enforcement to entity/relation inserts, whole-object deletes,
  role traversal, anonymous relations, nested fetches and stored frontend function
  calls. Scope copies of stored function bodies and nested helpers, rather than
  trusting a public-function flag or running them raw.
- Validate insert proposals against TypeDB function signatures, assign tenancy on
  the server, forbid implicit delete cascades, and validate post-write booleans
  before committing. Preserve deletion acknowledgement rows with mutation markers.
- Add guarded put and multi-mutation execution with typed transaction-local
  checkpoints and private per-row lineage. Roll back on later denial, partial
  row loss, post-write policy failure, unsupported stages or response limits.
- Add relation-based scope and complete-state creation policies for same-stage
  entity/relation graphs, hidden attributes (including subtypes), and explicitly
  context-aware schema functions with guarded object-return bindings.
- Deduplicate inherited block guards to avoid multiplying native policy plans in
  disjunctions. Close rejected/cancelled transactions explicitly, including
  transport cancellation, rather than relying solely on driver drop cleanup.
- Add real-TypeDB and loopback HTTP tests through authentication hooks and thin
  read/write services, plus Business's real-JWT schema-policy migration matrix;
  no unrestricted-safety claim.
- Align workspace Chrono minimums with the official parser's exact 0.4.44 pin;
  the parser also pins Regex 1.12.3. Revisit these constraints with upstream parser
  releases rather than forking its grammar or silently using a different parser.

## [0.3.1] — 2026-10-20

- Add explicit TenantTypeDBAdapter mode with immutable reviewed query contracts
  and mandatory request-local tenant/principal context; retain the trusted
  low-level executor for migrations and internal queries.
- Centralize full-query validation, trusted scope/claim checks, server-owned
  clocks/revisions, safe responses and pre-commit result-cardinality checks.
- Add adversarial tenant-policy unit tests and live concurrent two-tenant
  isolation, revocation and rollback tests; document authentication and
  trusted-contract review responsibilities.
- Support TLS and explicit credentials in the live test harness; validate the suite on TypeDB Cloud 3.13.6's free plan.
- Pin/test TypeDB driver and server 3.13.6; remove unsupported compatibility claims.
- Ignore literal/comment/variable keywords during automatic transaction routing.
- Add `QueryOptions`, explicit-mode `execute_query_with_options`, adapter options, and schema-batch options.
- Fail on answer/JSON-byte limits without committing writes; stop draining excess answers.
- Apply a whole-operation deadline and propagate column-access errors.
- Require every explicit schema file, preserve caller order, and reject duplicates/missing/empty files before opening the transaction.
- Document uncertain commit outcomes and migration from search paths/truncated results.


## [0.1.8] — 2026-06-07 — TypeDB Driver 3.11 Compatibility

### Changed
- **`typedb-driver`** bumped from `3.7.0` → `3.11.5`

### Migration

`TypeDBDriverFactory::connect` (and any direct driver construction) requires two API changes:

**Before:**
```rust
let options = DriverOptions::new(tls, None).map_err(|e| anyhow!(e))?;
TypeDBDriver::new(address, credentials, options).await?
```

**After:**
```rust
use typedb_driver::{Addresses, DriverOptions, DriverTlsConfig};

let tls_config = if tls { DriverTlsConfig::default() } else { DriverTlsConfig::disabled() };
let options = DriverOptions::new(tls_config);
let addresses = Addresses::try_from_address_str(address).map_err(|e| anyhow!(e))?;
TypeDBDriver::new(addresses, credentials, options).await?
```

**Key changes:**
- `DriverOptions::new` no longer accepts a `bool` — use `DriverTlsConfig::default()` (TLS on, system certs) or `DriverTlsConfig::disabled()`.
- `TypeDBDriver::new` now requires an `Addresses` object; pass a raw `&str` address via `Addresses::try_from_address_str`.
