# dog-schema

[![Crates.io](https://img.shields.io/crates/v/dog-schema.svg)](https://crates.io/crates/dog-schema)
[![Documentation](https://docs.rs/dog-schema/badge.svg)](https://docs.rs/dog-schema)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

**Schema definition and validation utilities for DogRS - JSON schema, validation, and type safety**

dog-schema provides powerful schema definition and validation capabilities for the DogRS ecosystem, enabling type-safe data handling with compile-time and runtime validation.

## Features

- **JSON Schema generation** - Automatic schema generation from Rust types
- **Runtime validation** - Validate data against schemas at runtime
- **Type safety** - Compile-time guarantees with procedural macros
- **Extensible validation** - Custom validators and constraints
- **Integration ready** - Works seamlessly with dog-core services

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-schema = "0.1.3"
```

### Basic Usage

```rust
use dog_schema::{Schema, Validate};
use serde::{Deserialize, Serialize};

#[derive(Schema, Serialize, Deserialize)]
struct User {
    #[schema(min_length = 1, max_length = 100)]
    name: String,
    
    #[schema(email)]
    email: String,
    
    #[schema(range(min = 0, max = 150))]
    age: u8,
}

// Generate JSON schema
let schema = User::json_schema();

// Validate data
let user_data = serde_json::json!({
    "name": "Alice",
    "email": "alice@example.com", 
    "age": 30
});

let user: User = User::from_json(&user_data)?;
```

## Schema Attributes

### String Validation
```rust
#[derive(Schema)]
struct TextData {
    #[schema(min_length = 5, max_length = 50)]
    title: String,
    
    #[schema(pattern = r"^[A-Z][a-z]+$")]
    name: String,
    
    #[schema(email)]
    email: String,
    
    #[schema(url)]
    website: String,
}
```

### Numeric Validation
```rust
#[derive(Schema)]
struct NumericData {
    #[schema(range(min = 0, max = 100))]
    percentage: f64,
    
    #[schema(minimum = 1)]
    count: u32,
    
    #[schema(multiple_of = 5)]
    step: i32,
}
```

### Collection Validation
```rust
#[derive(Schema)]
struct CollectionData {
    #[schema(min_items = 1, max_items = 10)]
    tags: Vec<String>,
    
    #[schema(unique_items)]
    categories: Vec<String>,
}
```

## Custom Validators

Create custom validation logic:

```rust
use dog_schema::{Schema, ValidationError, Validator};

struct PasswordValidator;

impl Validator<String> for PasswordValidator {
    fn validate(&self, value: &String) -> Result<(), ValidationError> {
        if value.len() < 8 {
            return Err(ValidationError::new("Password must be at least 8 characters"));
        }
        if !value.chars().any(|c| c.is_uppercase()) {
            return Err(ValidationError::new("Password must contain uppercase letter"));
        }
        Ok(())
    }
}

#[derive(Schema)]
struct Account {
    username: String,
    
    #[schema(validator = "PasswordValidator")]
    password: String,
}
```

## Integration with DogRS Services

Use schemas with dog-core services:

```rust
use dog_core::{DogService, TenantContext};
use dog_schema::Schema;

#[derive(Schema)]
struct CreateUserRequest {
    #[schema(min_length = 1)]
    name: String,
    
    #[schema(email)]
    email: String,
}

struct UserService;

#[async_trait]
impl DogService<CreateUserRequest, ()> for UserService {
    type Output = User;
    
    async fn create(&self, tenant: TenantContext, data: CreateUserRequest) -> Result<User> {
        // Data is already validated by the schema
        // Implement your business logic here
        Ok(User {
            id: generate_id(),
            name: data.name,
            email: data.email,
        })
    }
}
```

## JSON Schema Generation

Generate standard JSON schemas for API documentation:

```rust
use dog_schema::Schema;

#[derive(Schema)]
struct ApiResponse {
    success: bool,
    data: Option<serde_json::Value>,
    error: Option<String>,
}

// Generate JSON Schema
let schema = ApiResponse::json_schema();
println!("{}", serde_json::to_string_pretty(&schema)?);
```

Output:
```json
{
  "type": "object",
  "properties": {
    "success": {"type": "boolean"},
    "data": {"type": ["object", "null"]},
    "error": {"type": ["string", "null"]}
  },
  "required": ["success"]
}
```

## Validation Errors

Comprehensive error reporting:

```rust
match User::from_json(&invalid_data) {
    Ok(user) => println!("Valid user: {:?}", user),
    Err(errors) => {
        for error in errors {
            println!("Validation error at {}: {}", error.path, error.message);
        }
    }
}
```

## Architecture

dog-schema integrates with the DogRS ecosystem:

```
┌─────────────────┐
│   Your App      │  ← Business logic with validated types
└─────────────────┘
         │
    ┌────┴────┐
    │         │
┌───▼───┐ ┌──▼──────┐
│dog-   │ │dog-     │  ← Adapters
│axum   │ │schema   │
└───────┘ └─────────┘
    │         │
    └────┬────┘
         ▼
┌─────────────────┐
│   dog-core      │  ← Core abstractions
└─────────────────┘
```

## Examples

See `dog-examples/` for complete applications using schema validation:

- **blog-axum** - REST API with request/response validation

## License

MIT OR Apache-2.0

---

<div align="center">

**Made by [Jitpomi](https://github.com/Jitpomi)**

</div>
