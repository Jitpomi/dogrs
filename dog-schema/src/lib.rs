pub use dog_schema_macros::schema;

use dog_core::errors::DogError;
use serde_json::{json, Map, Value};

#[must_use = "call into_unprocessable_anyhow() to propagate errors"]
#[derive(Default)]
pub struct SchemaErrors {
    map: Map<String, Value>,
}

impl SchemaErrors {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_schema(&mut self, msg: impl Into<String>) {
        Self::push_to(&mut self.map, "_schema", msg);
    }

    pub fn push_field(&mut self, field: &str, msg: impl Into<String>) {
        Self::push_to(&mut self.map, field, msg);
    }

    fn push_to(map: &mut Map<String, Value>, key: &str, msg: impl Into<String>) {
        let msg = Value::String(msg.into());
        match map
            .entry(key.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
        {
            Value::Array(arr) => arr.push(msg),
            slot => {
                // Defensive: key held a non-array value — replace it.
                // This cannot happen via the public push_schema/push_field API.
                *slot = Value::Array(vec![msg]);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn into_unprocessable_anyhow(self, message: &str) -> anyhow::Error {
        DogError::unprocessable(message)
            .with_errors(Value::Object(self.map))
            .into_anyhow()
    }
}

pub fn unprocessable(message: &str, errors: Value) -> anyhow::Error {
    DogError::unprocessable(message)
        .with_errors(errors)
        .into_anyhow()
}

pub fn schema_error(message: &str, msg: impl Into<String>) -> anyhow::Error {
    unprocessable(message, json!({"_schema": [msg.into()]}))
}

pub mod schema_hooks;
pub use schema_hooks::{
    HookMeta, ResolveData, Rules, SchemaBuilder, SchemaHooksExt, ValidateData, WriteMethods,
};
