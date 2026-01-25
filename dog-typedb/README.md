# dog-typedb

[![Crates.io](https://img.shields.io/crates/v/dog-typedb.svg)](https://crates.io/crates/dog-typedb)
[![Documentation](https://docs.rs/dog-typedb/badge.svg)](https://docs.rs/dog-typedb)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

**TypeDB database integration for DogRS - adapters, utilities, and TypeQL query builders**

dog-typedb provides seamless TypeDB integration for the DogRS ecosystem with robust query handling, automatic transaction management, TypeDB inference and rules support, and TypeDB Studio-compatible response formatting.

## Features

- **Complete TypeQL support** - Match, fetch, insert, delete, and schema operations
- **TypeDB inference and rules** - Full support for TypeDB's reasoning capabilities
- **Automatic transaction management** - Smart routing for read/write/schema operations
- **TypeDB Studio compatibility** - Response format matches TypeDB Studio exactly
- **Production-ready** - Comprehensive error handling and robust query processing

## Quick Start

```bash
cargo add dog-typedb
```

### Basic Usage

```rust
use dog_typedb::execute_typedb_query;
use typedb_driver::{TypeDBDriver, Credentials, DriverOptions};
use std::sync::Arc;

// Setup TypeDB connection
let credentials = Credentials::new("admin", "password");
let options = DriverOptions::new(false, None)?;
let driver = Arc::new(
    TypeDBDriver::new("127.0.0.1:1729", credentials, options).await?
);

// Create database if it doesn't exist (optional)
if !driver.databases().all().await?.iter().any(|db| db.name() == "my-database") {
    driver.databases().create("my-database").await?;
}

// Load schema and functions (optional - only if you have .tql files)
use dog_typedb::load_schema_from_file;
load_schema_from_file(&driver, "my-database", &["schema.tql", "functions.tql"]).await?;

// Execute queries directly
let result = execute_typedb_query(&driver, "my-database", 
    "match $p isa person; limit 10;").await?;

println!("{}", result);
```

## Using TypeDBAdapter Directly

```rust
use dog_typedb::TypeDBAdapter;
use serde_json::json;

// Create adapter with your TypeDB state
let adapter = TypeDBAdapter::new(state);

// Read queries
let result = adapter.read(json!({
    "query": "match $p isa person, has name $n; limit 5;"
})).await?;

// Write queries  
let result = adapter.write(json!({
    "query": "insert $p isa person, has name \"Alice\";"
})).await?;

// Schema queries
let result = adapter.schema(json!({
    "query": "define person sub entity, owns name;"
})).await?;
```

## Integration with DogRS Services

For production applications, integrate with the DogRS service layer:

```rust
use dog_typedb::{TypeDBAdapter, TypeDBState as TypeDBStateTrait};
use dog_core::{DogService, TenantContext};
use dog_axum::DogAxum;
use serde_json::Value;
use std::sync::Arc;

// 1. Setup TypeDB state in your application
#[derive(Clone)]
pub struct TypeDBState {
    pub driver: Arc<TypeDBDriver>,
    pub database: String,
}

impl TypeDBStateTrait for TypeDBState {
    fn driver(&self) -> &Arc<TypeDBDriver> { &self.driver }
    fn database(&self) -> &str { &self.database }
}

// 2. Initialize in your app
let ax = DogAxum::new().await?;
TypeDBState::setup_db(ax.app.as_ref()).await?;
let state = ax.app.get::<Arc<TypeDBState>>("typedb")?;

// 3. Create service with TypeDB adapter
pub struct MyService {
    adapter: TypeDBAdapter<TypeDBState>,
}

impl MyService {
    pub fn new(state: Arc<TypeDBState>) -> Self {
        Self {
            adapter: TypeDBAdapter::new(state),
        }
    }
}

#[async_trait]
impl DogService<Value, ()> for MyService {
    async fn custom(
        &self,
        _ctx: &TenantContext,
        method: &str,
        data: Option<Value>,
        _params: (),
    ) -> Result<Value> {
        match method {
            "read" => self.adapter.read(data.unwrap()).await,
            "write" => self.adapter.write(data.unwrap()).await,
            _ => Err(anyhow::anyhow!("Unknown method: {}", method))
        }
    }
}
```

## Direct Query Execution

For lower-level usage, you can call `execute_typedb_query` directly:

```rust
// Match queries → conceptRows
let result = execute_typedb_query(&driver, "database",
    "match $p isa person, has name $n; limit 5;").await?;

// Fetch queries → conceptDocuments  
let result = execute_typedb_query(&driver, "database",
    "match $p isa person; fetch { \"person\": { $p.* } };").await?;

// Aggregation queries → conceptRows
let result = execute_typedb_query(&driver, "database", 
    "match $p isa person; reduce $count = count($p);").await?;
```

## TypeDB Studio Compatibility

All responses use the exact same format as TypeDB Studio:

```json
{
  "ok": {
    "queryType": "read",
    "answerType": "conceptDocuments",
    "answers": [
      {
        "data": {
          "name": "Alice",
          "person": {
            "name": "Alice",
            "age": 30
          }
        },
        "involvedBlocks": [0]
      }
    ],
    "query": "match $p isa person; fetch { \"name\": $p.name };",
    "warning": null
  }
}
```

## Error Handling

dog-typedb provides comprehensive error handling for all TypeDB operations:

```rust
match execute_typedb_query(&driver, "database", query).await {
    Ok(response) => println!("Success: {}", response),
    Err(e) => {
        eprintln!("TypeDB Error: {}", e);
        // Handle connection issues, syntax errors, etc.
    }
}
```

## Schema Loading

Load TypeDB schemas from files:

```rust
use dog_typedb::load_schema_from_file;

let result = load_schema_from_file(
    &driver, 
    "my-database", 
    &["schema.tql", "functions.tql"]
).await?;
```

## Architecture

dog-typedb integrates seamlessly with the DogRS ecosystem:

```
┌─────────────────┐
│   Your App      │  ← Business logic
└─────────────────┘
         │
    ┌────┴────┐
    │         │
┌───▼───┐ ┌──▼──────┐
│dog-   │ │dog-     │  ← Adapters
│axum   │ │typedb   │
└───────┘ └─────────┘
    │         │
    └────┬────┘
         ▼
┌─────────────────┐
│   dog-core      │  ← Core abstractions
└─────────────────┘
         │
         ▼
┌─────────────────┐
│   TypeDB        │  ← Database
└─────────────────┘
```

## Examples

Complete examples available in `dog-examples/`:

- **social-typedb** - Social network with TypeDB
- **fleet-queue** - Fleet management with TypeDB functions

## TypeDB Version Support

- **TypeDB 3.0+** - Full support including parameterized functions
- **TypeDB 2.x** - Core functionality supported

## License

MIT OR Apache-2.0

---

<div align="center">

**Made by [Jitpomi](https://github.com/Jitpomi)**

</div>
