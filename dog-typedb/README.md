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

### Tenant-safe shared-database queries

Use TenantTypeDBAdapter for customer-supplied queries. The existing TypeDBAdapter
and free execution functions are **trusted, unscoped executors**. This is an
explicit migration, not an automatic change to the old API's security guarantees.

The tenant-safe path preserves the DogRS application structure:

1. The frontend renders complete TypeQL and sends only { "query": "..." }.
   Custom service methods remain read and write.
2. Before hooks authenticate the real caller, check current tenant membership,
   and issue a request-local TenantQueryContext from trusted identity data.
3. Services delegate to adapter.read(&context, data) or
   adapter.write(&context, data). The **adapter itself** enforces the query
   contract; skipping an application validation hook cannot skip those checks.
4. Reviewed query sources call the application's TypeQL permission functions.
   Those functions—not Rust role checks—decide resource access, expiry,
   revocation, policy eligibility and separation of duties.

Application construction uses the same trusted query source shipped to the
frontend, not a second copy of its business logic:

~~~rust,no_run
use std::sync::Arc;
use dog_typedb::{adapter::TypeDBState, QueryContract, QueryMethod,
    TenantQueryPolicy, TenantTypeDBAdapter, TypeDBAdapter};

// source is the application-owned frontend query source loaded at startup,
// for example with include_str!, never text supplied by a customer.
fn configure<T: TypeDBState>(
    state: Arc<T>, source: &str,
) -> anyhow::Result<(TenantTypeDBAdapter, TenantQueryPolicy)> {
    let policy = TenantQueryPolicy::new([
        QueryContract::new(QueryMethod::Read, source),
    ])?;
    let adapter = TypeDBAdapter::new(state).tenant_scoped(policy.clone());
    Ok((adapter, policy))
}
~~~

After authentication, the before hook issues context and stores it in server-owned
request params (for example in Arc), never in the request body:

~~~rust,no_run
use dog_core::tenant::TenantContext;
use dog_typedb::{TenantQueryPolicy, TenantQueryContext};

fn after_authentication(
    policy: &TenantQueryPolicy,
    verified_tenant_context: TenantContext,
    verified_principal_id: String,
    verified_string_claims: Vec<(String,String)>,
) -> anyhow::Result<TenantQueryContext> {
    policy.authenticated_context(
        &verified_tenant_context, &verified_principal_id, verified_string_claims,
    )
}
~~~

The custom read/write service then passes that scope and the frontend's original
query to the adapter. There is no unscoped overload or schema method on the
tenant-safe adapter.

**Authentication boundary:** authenticated_context does NOT authenticate its
arguments. Calling it with an unverified tenant header or body-selected principal
is insecure. Contexts cannot be deserialized, have private fields, and are bound
to their issuing policy instance. Use a clone of that policy for the adapter.
Recheck membership/revocation per request; do not cache contexts as sessions.

Contracts match the **whole query structure**, not an authorization-function
substring. Only declared literal positions may vary. They require unquoted
@@tenant_id@@ and @@identity_id@@ placeholders and compare submitted values
against the authenticated context. Repeated literals must agree. The optional
@@now@@ and @@revision@@ literals are replaced with server UTC time and a fresh
UUID. Other placeholders require QueryParameter declarations: strings, bounded
text, identifiers, UTC datetimes, or authenticated claims. These are literal/input
bounds, not business permission decisions.

Extra request properties, unreviewed queries, changed predicates, appended
statements, wrong methods, foreign contexts and forged scope are rejected.
Policies compile once at startup and are immutable. Request scope is never stored
in a shared adapter. Read/write transaction modes remain forced.

Use one_answer() on contracts that must affect exactly one result. Zero answers
return unavailable/not-authorized; multiple answers fail **before commit**.
Answer/byte/deadline limits still apply. Responses retain the DogRS envelope but
remove ok.query. Driver errors are sanitized because they can embed expanded
queries and trusted claims. A failed write may still have an uncertain commit
outcome: reconcile rather than blindly retry.

**Contract-review boundary:** contracts are trusted application configuration.
The library cannot infer every application's schema or prove that a trusted query
calls the right policy function for every affected record. Registering a query
that selects all tenants, exposes credentials, changes grants without
authorization, or omits resource checks is unsafe. TenantContext and TypeDB
functions are not automatic row-level security for unrestricted TypeQL.
Changing query structure requires deploying its reviewed contract; this is not
an unrestricted customer query console.

## Registry-free schema policies — opt-in

`isolation::SchemaPolicyAdapter` is a separate opt-in executor, not a change to
the existing adapter or the published 0.3.1 query-contract API. It has **no query
registry**. The frontend sends a complete `{ "query": "..." }`, services retain
custom `read`/`write` methods, and authentication hooks supply a non-deserializable
`AuthenticatedScope` built from verified tenant/identity data. Never construct
that scope from an unverified tenant header or a body-selected identity.

The TypeQL schema declares ownership and policy, not allowed query strings:

```typeql
entity note
  @meta("dog.security.tenant", "tenant-key")
  @meta("dog.security.read", "can_read_note")
  @meta("dog.security.insert", "can_insert_note")
  @meta("dog.security.delete", "can_delete_note"),
  owns tenant-key @card(1),
  owns title @card(1) @meta("dog.security.update", "can_update_title");
```

The corresponding schema functions have these signatures:

```text
can_read_note(tenant: string, actor: string, now: datetime, object: note) -> boolean
can_update_title(tenant: string, actor: string, now: datetime, object: note, proposed: string) -> boolean
can_insert_note(tenant: string, actor: string, now: datetime, title: string) -> boolean
can_delete_note(tenant: string, actor: string, now: datetime, object: note) -> boolean
```

They implement membership, current permission, revocation, proposed-value checks,
and business rules in TypeDB. Update functions receive the actual typed proposed
literal; another ownership can use its own value type and policy function.
All authorization functions must use `return check`, with the first three
parameters named/typed `tenant: string, actor: string, now: datetime`. The compiler
checks this contract; a denial must produce a boolean, not erase a result row.
For inserts, the remaining parameter names must exactly cover the proposed
attribute/role names, with no extra or missing fields. Role players must already
be matched and guarded; tenant ownership is inserted by the framework, never
selected by the caller. The runnable model is in `tests/fixtures/schema_policy.tql`.

The framework parses the entire frontend query with the official TypeQL 3.13.4
parser, forces the transaction mode, reads policy metadata inside that transaction,
and injects tenant constraints plus mandatory policy calls **before** mutations.
Tenant/actor/current UTC time are driver `given` values, not interpolated literals.
Object access requires one unambiguous tenant value. Updating the tenant attribute
or its type hierarchy is forbidden, even if an ownership has update metadata.
There is no raw-execution fallback, client-selected database or schema method.
Full-stream response budgets are checked before commit; a limit failure rolls
back mutations. Every surviving guarded object's read permission is checked
again when a write returns projected data. These mandatory booleans are checked
before commit, rather than filtering already-mutated rows out of the response.
Queries, trusted inputs and raw driver diagnostics are not
returned to the caller. Private error causes retain the execution phase for
trusted diagnostics. A driver-confirmed STC2 commit rejection returns 409;
commit deadlines and transport failures still report an uncertain outcome.
No failed write is automatically replayed.

**Supported:** guarded entity/relation reads and role traversal, anonymous
relation patterns, nested correlated fetch queries, conjunction/disjunction/
negation, value expressions, select/sort/limit/offset/distinct, literal ownership
updates, new entity/relation inserts, guarded `put`, and whole-object deletes. `isa!` is exact;
ordinary `isa` is accepted for schema leaves only. Inserts/updates require explicit
fetch projections. Deletes without fetch receive a private mutation marker and
a minimal acknowledgement, so deleting the last concept does not erase its row.

**Multi-mutation transactions:** each mutation has a transaction-local typed-row
checkpoint, including mandatory post-write authorization booleans. Subsequent
stages receive actual TypeDB concepts through driver `given` rows, not JSON
reconstructions, and never replay earlier writes. Private lineage identifiers
detect a later guard/filter discarding any earlier mutation row, including
partial loss within a multi-row batch; the entire transaction then rolls back.
Checkpoints obey row/byte/time budgets. The final projection must retain every
checkpoint lineage. Heterogeneous bindings and mutation pipelines without an
explicit final fetch fail closed. `put` uses the same complete proposal policy
as insert even when it finds an existing object, injects tenant ownership into
the upsert pattern, and checks read authorization before commit.

Compiler-generated schema-introspection results are reused only inside that
transaction while its schema lock is held, including across checkpoints. This
avoids repeated database round trips consuming the operation deadline. The
metadata store is byte-bounded and discarded after the request; frontend queries,
policy outcomes, membership and application data are never cached there. The
default operation deadline remains 30 seconds.

**Frontend function calls:** the compiler reads stored TypeQL definitions from
the database schema and emits request-local, context-parameterized copies. It
scopes the function bodies and nested helper calls before they run. Scalar,
value-stream and object-stream returns are supported, including `return count`
and other scalar return reducers. A global stored function is not run globally
just because the frontend calls it. Functions reading unclassified credential
objects fail closed. There is no application-maintained function/query registry.

Authorization functions named by schema policy metadata remain privileged
database policy code and are invoked by the framework. Their correctness is a
trusted schema-author responsibility; they are not automatically made public
or called unmodified when a frontend names them.

**Relation-based ownership:** a type can declare
`@meta("dog.security.scope", "scope_function")` instead of a tenant attribute.
This function takes `(tenant: string, actor: string, now: datetime, object: TYPE)`
and uses `return check`. It must verify ownership through the model's relations
and reject cross-tenant objects. Both scope and read policies are mandatory on
reads and post-write checks. A relation scope deliberately governs the declared
type hierarchy; model authors must account for its subtypes. Attribute-only
policies keep their exact-leaf restrictions.

**Same-stage graphs:** types using relation scope can also declare
`@meta("dog.security.create", "create_function")`, with the same context/object
signature and boolean `return check`. Every new object, including anonymous
ownership relations, needs this policy. The native insert/put constructs the
complete graph first; mandatory create, scope and read checks then validate all
objects before commit. Policies and schema constraints must validate the complete
created state, not just existence. An orphan, foreign player, missing policy or
failed check rolls back the entire graph. Objects using proposal-only insertion
policies cannot be mixed into this mode. Existing objects in a `put` must also
satisfy its create policy; this is intentionally more restrictive than raw TypeDB.

**Private attributes:** `@meta("dog.security.hidden", "true")` on an attribute
type forbids frontend matching, projection and mutation of it and its subtypes.
This lets a tenant-scoped directory object coexist with credentials without
exposing those credentials. Direct attribute enumeration and wildcard fetches
remain forbidden. Trusted authentication resolvers use the separate raw adapter.

**Context-aware domain functions:** a stored function explicitly annotated
`@meta("dog.security.context", "true")` is privileged schema code, not a raw
frontend-defined function. Its first three arguments must be the server-owned
tenant, actor and clock; the frontend supplies only the remaining domain args.
Value results may be projected; object results must first be bound to variables
and pass normal scope/read guards. Direct unguarded object-result projection is
rejected. All unannotated stored functions continue to be structurally scoped,
including nested helpers. This is a schema-author trust boundary, not a proof
that arbitrary policy function bodies are correct, and not a query registry.

Pre-commit errors explicitly close transactions. If a transport cancels a request,
a bounded independent close task finishes cleanup rather than relying only on the
driver's asynchronous drop notification. Commit cancellation still has an unknown
outcome and must be reconciled, never blindly retried.

**Deletion safety:** an object with incoming relations cannot be deleted through
an implicit cascade. Delete authorized relations first. Relation deletion also
requires every actual role player to be one of the explicitly guarded objects.
A legacy cross-tenant edge therefore blocks deletion instead of being silently
modified. Role capabilities are derived from the exported TypeQL schema.

**Rejected, not silently bypassed:** dynamic types/non-leaf subtype queries,
direct attribute enumeration/deletion, ownership/link-delete forms, optional
exports, wildcard projections, caller-supplied inline function definitions/blocks,
recursive function cycles, collection reducers, explicit reduce stages,
multi-field updates,
repeated proposal-only insert fields/roles, and proposal-only inserts referencing
objects newly created in that same stage. State-based graph creation is described
above. These restrictions never fall back to raw execution.
Query/schema sizes, nesting, function expansion, patterns, response size and
operation time are bounded.

**Trust boundary:** schema policy authors and schema deployment credentials are
trusted. A type's read policy grants access to that object's attributes; this
version has no conditional field-read policies; hidden attributes are always
excluded. Do not classify whole credential objects as publicly readable.
The executor does not prove the correctness of
policy function bodies. Policy decisions use the transaction snapshot; this is
not a guarantee that an in-flight operation is cancelled by a concurrent revocation.

Consuming applications must validate their schema policies and real authenticated
request paths before replacing older safeguards. This is not unrestricted TypeQL
support or automatic safety for an unreviewed schema. Business's candidate migration
exercises frontend queries, JWT authentication, multi-object puts, domain functions,
private fields, concurrency and rollback against a real database and HTTP server.

The live matrix includes real ephemeral loopback HTTP through `dog-transport`,
authentication hooks, thin custom read/write services, and real TypeDB. Fixture
credentials are deliberately not a production JWT provider. Coverage includes
concurrent tenants, nested helpers/counts, object returns, relation traversal,
proposal validation, denied writes, empty scalar results, revocation, mutation
acknowledgements, immutable tenancy, legacy cross-tenant edges and rollback.
This is not a load/failover certification or proof of unrestricted TypeQL safety.

The TypeQL parser currently pins Chrono 0.4.44 and Regex 1.12.3. Workspace Chrono
minimums were relaxed from 0.4.45 to 0.4.44 to resolve those upstream constraints;
this is a compatibility constraint, not a new application architecture.

## Verification

Unit tests cover lexical routing, exact byte accounting, early stream termination, error propagation, input limits, and strict file loading. Live tests cover read/write/schema permissions, schema rollback, write rollback after result limits, explicit missing-file rejection, and schema-lock deadlines followed by connection reuse.

Tenant tests also cover forged tenant/principal/claims, missing context bindings,
cross-policy contexts, wrong methods, appended/altered queries, escaped literal
injection, server-owned clocks/revisions, and response cardinality. Live tests run
concurrent two-tenant reads, policy-denied writes, membership revocation and
pre-commit rollback of unexpected bulk results in a uniquely generated database.

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
