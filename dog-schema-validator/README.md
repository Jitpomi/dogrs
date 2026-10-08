# dog-schema-validator

An optional adapter from Serde and the `validator` crate to DogRS structured
validation errors. Its public entry point is `validate::<T>(&Value, message)`;
`T` must implement `DeserializeOwned + validator::Validate`.

```rust
use serde::Deserialize;
use validator::Validate;
use serde_json::json;

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct User {
    #[validate(email)]
    email: String,
}

let user = dog_schema_validator::validate::<User>(
    &json!({"email": "alice@example.com"}), "Invalid user",
)?;
assert_eq!(user.email, "alice@example.com");
# Ok::<(), anyhow::Error>(())
```

For generated service hooks:

```rust
#[dog_schema::schema(service = "users", backend = "validator")]
mod users {
    use serde::Deserialize;
    use validator::Validate;

    #[create]
    #[derive(Deserialize, Validate)]
    #[serde(deny_unknown_fields)]
    pub struct Create {
        #[validate(email)]
        pub email: String,
    }
    #[patch]
    #[derive(Deserialize, Validate)]
    #[serde(deny_unknown_fields)]
    pub struct Patch {
        #[validate(email)]
        pub email: Option<String>,
    }
}
let mut builder = dog_core::DogApp::<serde_json::Value, ()>::builder();
users::register(&mut builder)?;
# Ok::<(), anyhow::Error>(())
```

Both deserialization and constraint failures become `DogError` 422 responses.
Nested validation errors use paths such as `profile.name` and `tags[0].email`.
Deserialization failures use a generic `_schema` message to avoid exposing input
values or arbitrary custom-deserializer messages. Explicit custom validator
messages are application-controlled: do not place secrets in them.

Serde controls unknown fields, aliases, defaults and null handling. The direct
`validate` function returns the parsed value. Generated hooks only validate; they
do not replace the original JSON with that parsed value or materialize Serde
defaults into it. Use application resolvers for normalization. Built-in `dog`
field rules are rejected with this backend to prevent silently ignored checks.

This adapter does not generate JSON Schema, perform authorization, query databases,
or guarantee business invariants. Applications can use other validation libraries
through `dog-schema`'s closure hooks instead. The documented source API is 0.3.0;
verify publication before choosing a registry version.
