pub use dog_schema_macros::schema;

use dog_core::errors::DogError;
use serde_json::{json, Map, Value};

#[derive(Default)]
pub struct SchemaErrors {
    map: Map<String, Value>,
}

impl SchemaErrors {
    pub fn push_schema(&mut self, msg: impl Into<String>) {
        Self::push_to(&mut self.map, "_schema", msg);
    }

    pub fn push_field(&mut self, field: &str, msg: impl Into<String>) {
        Self::push_to(&mut self.map, field, msg);
    }

    fn push_to(map: &mut Map<String, Value>, key: &str, msg: impl Into<String>) {
        let msg = Value::String(msg.into());
        match map.get_mut(key) {
            Some(Value::Array(arr)) => arr.push(msg),
            Some(_) => {
                map.insert(key.to_string(), Value::Array(vec![msg]));
            }
            None => {
                map.insert(key.to_string(), Value::Array(vec![msg]));
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
    DogError::unprocessable(message).with_errors(errors).into_anyhow()
}

pub fn schema_error(message: &str, msg: impl Into<String>) -> anyhow::Error {
    unprocessable(message, json!({"_schema": [msg.into()]}))
}
