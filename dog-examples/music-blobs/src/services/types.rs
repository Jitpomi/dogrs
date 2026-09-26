use std::collections::HashMap;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MusicParams {
    pub provider: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub method: String,
    pub path: String,
    pub raw_query: Option<String>,
}
