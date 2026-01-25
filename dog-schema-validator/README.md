# dog-schema-validator

[![Crates.io](https://img.shields.io/crates/v/dog-schema-validator.svg)](https://crates.io/crates/dog-schema-validator)
[![Documentation](https://docs.rs/dog-schema-validator/badge.svg)](https://docs.rs/dog-schema-validator)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

**Advanced validation utilities for DogRS schemas - runtime validation, constraints, and error handling**

dog-schema-validator provides advanced validation capabilities that extend dog-schema with runtime validation, custom constraints, and comprehensive error handling for production applications.

## Features

- **Runtime validation** - Validate data against schemas at runtime
- **Custom constraints** - Define complex validation rules
- **Detailed error reporting** - Comprehensive validation error messages
- **Performance optimized** - Fast validation for high-throughput applications
- **Integration ready** - Works seamlessly with dog-schema and dog-core

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-schema-validator = "0.1.3"
```

### Basic Usage

```rust
use dog_schema_validator::{Validator, ValidationRules, ValidationError};
use dog_schema::Schema;
use serde::{Deserialize, Serialize};

#[derive(Schema, Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
    age: u8,
}

// Create validation rules
let rules = ValidationRules::new()
    .field("name")
        .min_length(1)
        .max_length(100)
        .pattern(r"^[A-Za-z\s]+$")
    .field("email")
        .email()
        .required()
    .field("age")
        .range(0..=150);

// Validate data
let user_data = serde_json::json!({
    "name": "Alice Smith",
    "email": "alice@example.com",
    "age": 30
});

match rules.validate(&user_data) {
    Ok(()) => println!("Validation passed"),
    Err(errors) => {
        for error in errors {
            println!("Error: {} at {}", error.message, error.field_path);
        }
    }
}
```

## Advanced Validation Rules

### String Validation
```rust
use dog_schema_validator::ValidationRules;

let rules = ValidationRules::new()
    .field("username")
        .min_length(3)
        .max_length(20)
        .pattern(r"^[a-zA-Z0-9_]+$")
        .not_empty()
    .field("password")
        .min_length(8)
        .contains_uppercase()
        .contains_lowercase()
        .contains_digit()
        .contains_special_char();
```

### Numeric Validation
```rust
let rules = ValidationRules::new()
    .field("price")
        .positive()
        .decimal_places(2)
        .range(0.01..=999999.99)
    .field("quantity")
        .integer()
        .min(1)
        .max(1000);
```

### Collection Validation
```rust
let rules = ValidationRules::new()
    .field("tags")
        .array()
        .min_items(1)
        .max_items(10)
        .unique_items()
        .each_item(|item_rules| {
            item_rules.string().min_length(1).max_length(50)
        });
```

## Custom Validators

Create complex validation logic:

```rust
use dog_schema_validator::{CustomValidator, ValidationContext, ValidationError};

struct BusinessRuleValidator;

impl CustomValidator for BusinessRuleValidator {
    fn validate(&self, value: &serde_json::Value, context: &ValidationContext) -> Result<(), ValidationError> {
        // Access other fields through context
        let user_type = context.get_field("user_type")?;
        let age = context.get_field("age")?.as_u64().unwrap_or(0);
        
        if user_type == "premium" && age < 18 {
            return Err(ValidationError::new(
                "Premium users must be 18 or older"
            ));
        }
        
        Ok(())
    }
}

let rules = ValidationRules::new()
    .field("user_type")
        .string()
        .one_of(&["basic", "premium"])
    .custom_validator(BusinessRuleValidator);
```

## Conditional Validation

Validate fields based on other field values:

```rust
let rules = ValidationRules::new()
    .field("country")
        .string()
        .required()
    .field("postal_code")
        .when("country", "US")
            .pattern(r"^\d{5}(-\d{4})?$")
        .when("country", "CA")
            .pattern(r"^[A-Z]\d[A-Z] \d[A-Z]\d$")
        .when("country", "UK")
            .pattern(r"^[A-Z]{1,2}\d[A-Z\d]? \d[A-Z]{2}$");
```

## Error Handling

Comprehensive error reporting with field paths:

```rust
use dog_schema_validator::{ValidationError, ErrorSeverity};

match rules.validate(&data) {
    Ok(()) => println!("All validations passed"),
    Err(errors) => {
        for error in errors {
            match error.severity {
                ErrorSeverity::Error => {
                    eprintln!("ERROR at {}: {}", error.field_path, error.message);
                }
                ErrorSeverity::Warning => {
                    println!("WARNING at {}: {}", error.field_path, error.message);
                }
            }
        }
    }
}
```

## Integration with DogRS Services

Use with dog-core services for automatic validation:

```rust
use dog_core::{DogService, TenantContext};
use dog_schema_validator::{Validated, ValidationRules};

#[derive(Validated)]
#[validation_rules = "user_validation_rules"]
struct CreateUserRequest {
    name: String,
    email: String,
    age: u8,
}

fn user_validation_rules() -> ValidationRules {
    ValidationRules::new()
        .field("name").min_length(1).max_length(100)
        .field("email").email().required()
        .field("age").range(13..=120)
}

struct UserService;

#[async_trait]
impl DogService<CreateUserRequest, ()> for UserService {
    type Output = User;
    
    async fn create(&self, tenant: TenantContext, data: CreateUserRequest) -> Result<User> {
        // Data is automatically validated before reaching this method
        Ok(User::new(data.name, data.email, data.age))
    }
}
```

## Performance Features

### Validation Caching
```rust
use dog_schema_validator::CachedValidator;

let validator = CachedValidator::new(rules)
    .with_cache_size(1000)
    .with_ttl(Duration::from_secs(300));

// Repeated validations of similar data structures are cached
let result = validator.validate(&data)?;
```

### Async Validation
```rust
use dog_schema_validator::AsyncValidator;

let validator = AsyncValidator::new(rules);

// For I/O bound validations (database lookups, API calls)
let result = validator.validate_async(&data).await?;
```

## Validation Middleware

Use with dog-axum for automatic request validation:

```rust
use dog_axum::validation::ValidatedJson;
use dog_schema_validator::ValidationRules;

async fn create_user(
    ValidatedJson(user_data): ValidatedJson<CreateUserRequest>
) -> Result<Json<User>, AppError> {
    // user_data is already validated
    let user = user_service.create(user_data).await?;
    Ok(Json(user))
}
```

## Architecture

dog-schema-validator extends the DogRS ecosystem:

```
┌─────────────────┐
│   Your App      │  ← Business logic with validated data
├─────────────────┤
│   dog-axum      │  ← HTTP validation middleware
├─────────────────┤
│ dog-schema-     │  ← Advanced validation rules
│   validator     │
├─────────────────┤
│   dog-schema    │  ← Schema definitions
├─────────────────┤
│   dog-core      │  ← Core service traits
└─────────────────┘
```

## Examples

See `dog-examples/` for complete applications:

- **blog-axum** - REST API with comprehensive validation
- **social-typedb** - Social network with user data validation

## License

MIT OR Apache-2.0

---

<div align="center">

**Made by [Jitpomi](https://github.com/Jitpomi)**

</div>
