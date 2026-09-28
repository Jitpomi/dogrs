# TypeDB social example

A loopback playground for a social graph, using Axum, `dog-transport` and
`dog-typedb`. Services for persons, organizations, groups, posts and comments
expose explicit TypeQL `read` and `write` operations.

This is a trusted local query tool. It is not a production identity service or a
tenant-isolated API. `TenantContext` does not automatically add tenant predicates
to arbitrary TypeQL. Never expose raw queries to untrusted callers.

## Run

Use an existing disposable TypeDB 3.13.6 server, or start one locally:

```sh
docker run --rm --name dogrs-social-db -p 127.0.0.1:1729:1729 typedb/typedb:3.13.6
```

From the workspace root, in another terminal:

```sh
TYPEDB_ADDRESS=127.0.0.1:1729 TYPEDB_DATABASE=social-network cargo run -p social-typedb
```

Open `http://127.0.0.1:3036`. A new database receives `src/schema.tql`; it starts
empty. Its schema models profiles, posts, relationships, organizations and groups.
The files `sample_data.tql` and `load_sample_data.sh` are optional local fixtures;
review their database/connection settings before using them.

A bounded raw read through the persons service:

```sh
curl --fail http://127.0.0.1:3036/persons \
  -H 'Content-Type: application/json' -H 'x-service-method: read' \
  -d '{"query":"match $p isa person; limit 10; fetch {\"person\": $p.*};"}'
```

`x-service-method: write` selects a write transaction. Missing request payloads
return errors. The framework's query limits apply, and transactions must complete
successfully before a write commits. Do not blindly retry a write after an
ambiguous commit response; use an application operation ID to reconcile it.

## Configuration and migrations

The example reads `TYPEDB_ADDRESS`, `TYPEDB_DATABASE`, `TYPEDB_USERNAME`,
`TYPEDB_PASSWORD` and optional `TYPEDB_TLS`. Remote connections require explicit
credentials and verified TLS. Only loopback permits the default local credentials.

Existing databases are left unchanged unless you explicitly set
`TYPEDB_INIT_SCHEMA=1`. Treat that as a migration operation and test against a
copy first. Schema loading is not run on every ordinary restart.

Application composition, hooks, channels and per-service modules follow the
[common layout](../../docs/application-structure.md). Database connection and
schema initialization are in `src/typedb.rs`; provider-independent service
interfaces remain in DogRS. See the [TypeDB guide](../../dog-typedb/README.md)
for the current API, limits and tested server version.
