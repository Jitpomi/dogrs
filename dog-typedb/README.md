# dog-typedb

TypeDB integration for DogRS, using the **3.13.6 Rust driver**, tested against **TypeDB Server 3.13.6**. This release does not claim TypeDB 2.x or general 3.x compatibility. No web server is required.

## Execute a query

```rust,no_run
use dog_typedb::{execute_query_with_options, QueryOptions, TransactionType, TypeDBDriverFactory};

# async fn example() -> anyhow::Result<()> {
let password = std::env::var("TYPEDB_PASSWORD")?;
let driver = TypeDBDriverFactory::connect("db.example.com:1729", "app-user", &password, true).await?;
let result = execute_query_with_options(
    &driver,
    "app-database",
    "match $p isa person; limit 10; fetch { 'id': iid($p) };",
    TransactionType::Read,
    &QueryOptions::default(),
).await?;
println!("{result}");
# Ok(())
# }
```

`TypeDBDriverFactory::connect` accepts explicit credentials and a TLS setting. Use TLS for remote connections. `connect_default` uses the known `admin`/`password` credentials and plaintext; it is only for disposable local test servers. Connection establishment uses driver defaults; the query-operation deadline begins after a driver exists.

`TypeDBAdapter::new(Arc<T>)` accepts a state implementing `dog_typedb::adapter::TypeDBState`, whose methods return `&Arc<typedb_driver::TypeDBDriver>` and the database name. The adapter itself is not generic. Configure it with `adapter.with_options(QueryOptions { .. })?`. Its `read`, `write`, and `schema` methods accept a JSON object with a `query` string and **force the named transaction mode**. TypeDB enforces operation permissions; a write method cannot elevate itself to a schema transaction.

The low-level `execute_typedb_query` helper automatically chooses a transaction using lexical pipeline analysis. Quoted strings, escaped quotes, comments, and variable names are excluded from stage detection. This is a routing convenience, not a full TypeQL parser or authorization mechanism. Use explicit transaction modes for application endpoints.

## Bounds and transaction behavior

Default `QueryOptions`:

| Setting | Default |
|---|---:|
| Query bytes (UTF-8) | 1 MiB |
| Returned answers | 10,000 |
| Encoded JSON response bytes, including query/envelope | 8 MiB |
| Whole operation deadline, including commit | 30 seconds |

Limits must be positive; the deadline cannot exceed one day. Customize limits for your workload and implement request/concurrency admission in your host application.

Exceeding the answer or byte limit returns an error, stops polling the answer stream, and does not commit a write/schema transaction. There is no truncated success. The row limit inspects at most one extra answer to detect overflow. Byte accounting includes JSON escaping and response metadata and avoids allocating a second serialized copy of the response. The driver still materializes incoming rows/documents and may buffer network data; these limits are **not a hard cap on driver/server process memory**. Prefer paginated queries and avoid individually enormous documents.

Column access and stream errors propagate. Missing optional column values remain absent. Write/schema operations consume and validate the complete result before calling commit. Transactions that exit before commit are dropped and closed by the driver. Timeout or connection failure during commit can leave the outcome unknown: reconcile using an application operation ID before retrying. The wrapper does not automatically retry writes.

Responses use a DogRS envelope with `ok.queryType`, `answerType` (`ok`, `conceptRows`, or `conceptDocuments`), `answers`, `query`, and `warning`. This is not a promise of exact TypeDB Studio wire-format compatibility. Row concept values retain the crate's existing display-string representation; use fetch documents when you need structured JSON values. Queries are echoed in successful responses, so do not expose raw admin query endpoints or log sensitive query text indiscriminately.

## Schema files

```rust,no_run
# async fn example(driver: &typedb_driver::TypeDBDriver) -> anyhow::Result<()> {
dog_typedb::load_schema_from_file(driver, "app-database", &[
    "migrations/001-schema.tql",
    "migrations/002-functions.tql",
]).await?;
# Ok(())
# }
```

Every explicit path is required. Files may have any name and execute in caller order in **one schema transaction**. A directory expands to required `schema.tql`, followed by optional `functions.tql`. Missing paths, duplicate files, empty files, invalid UTF-8, and non-regular files fail. All sources are read and validated before opening the transaction. Supply 1–64 paths; the query-byte budget applies to the combined source contents. The response-byte budget applies to the entire batch response.

`load_schema_with_options` allows custom limits. Schema loading does not suppress "already exists" errors or track migration versions. Version and coordinate migrations in your application; avoid running schema initialization on every production startup.

**Migration:** paths used to be treated as alternative search locations. They are now explicit required inputs. Pass only the selected files/directory. `loadedFiles` reports the supplied/expanded file paths. Clients that previously accepted truncated answers must now handle limit errors and paginate.

## Service and tenant integration

`TypeDBService::with_handlers` delegates CRUD work to application-supplied handlers. It does not invent your schema or queries. Handlers receive `TenantContext`, but database selection and row-level tenant filtering are application responsibilities. Raw TypeQL must be limited to authorized callers. Do not interpolate untrusted values into query text without proper TypeQL encoding/validation.

## Verification

Unit tests cover lexical routing, exact byte accounting, early stream termination, error propagation, input limits, and strict file loading. Live tests cover read/write/schema permissions, schema rollback, write rollback after result limits, explicit missing-file rejection, and schema-lock deadlines followed by connection reuse.

Run against a disposable local **3.13.6** server:

```sh
TYPEDB_ADDRESS=127.0.0.1:1729 cargo test -p dog-typedb --all-targets --locked -- --include-ignored
```

For a hosted server, securely populate `TYPEDB_ADDRESS`, `TYPEDB_USERNAME`, and
`TYPEDB_PASSWORD` in the test process environment, then run:

```sh
cargo test -p dog-typedb --test live_database --locked -- --ignored
```

Remote connections require explicit credentials and always use TLS with system
trust roots. Only loopback addresses allow the local default credentials and
plaintext connection; `TYPEDB_TLS=true` also enables TLS for loopback.

The live test creates and deletes a randomly named database, so the configured
user needs database creation/deletion permissions. It never selects an existing
application database. CI runs it explicitly against a local server; this alone
does not establish TypeDB Cloud validation. Provider failover, backup restoration,
and deployment-specific capacity are separate operational tests.

On 2026-09-27, the live suite also passed against TypeDB Cloud 3.13.6 over TLS
on its free single-node GCP plan (2 burstable vCPUs, 4 GB RAM, 10 GB storage).
The test completed in 9.44 seconds, including deleting its temporary database.
This validates the covered transaction and query-limit behaviors on that hosted
configuration; it is not a load, failover, or backup-restoration certification.
