use std::collections::HashMap;
use serde_json::Value;
use crate::tenant::TenantContext;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DogTransportKind {
    Http,
    Sse,
    Grpc,
    WebSocket,
    Cli,
    Internal,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DogMethod {
    Find,
    Get,
    Create,
    Update,
    Patch,
    Remove,
    Custom(String),
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DogParams {
    #[serde(flatten)]
    pub inner: HashMap<String, Value>,
}

impl DogParams {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn deserialize<P>(self) -> Result<P, serde_json::Error>
    where
        P: serde::de::DeserializeOwned,
    {
        let val = Value::Object(self.inner.into_iter().collect());
        match serde_json::from_value(val) {
            Ok(v) => Ok(v),
            Err(err) => {
                if let Ok(v) = serde_json::from_value(Value::Null) {
                    Ok(v)
                } else {
                    Err(err)
                }
            }
        }
    }
}

impl From<HashMap<String, Value>> for DogParams {
    fn from(inner: HashMap<String, Value>) -> Self {
        Self { inner }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DogRequest {
    pub request_id: Option<String>,
    pub transport: DogTransportKind,
    pub service: String,
    pub method: DogMethod,
    pub id: Option<String>,
    pub tenant: TenantContext,
    pub params: DogParams,
    pub payload: Option<Value>,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DogResponse {
    pub payload: Option<Value>,
    pub metadata: HashMap<String, Value>,
}
