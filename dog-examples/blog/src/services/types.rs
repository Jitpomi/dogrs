use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BlogParams {
    pub provider: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub method: String,
    pub path: String,
    pub raw_query: Option<String>,
}

#[derive(Default)]
pub struct BlogState {
    pub posts_by_tenant: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
    pub authors_by_tenant: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
}
