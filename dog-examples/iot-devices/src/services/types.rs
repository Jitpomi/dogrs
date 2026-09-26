#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DemoParams {
    pub provider: String,
    pub headers: std::collections::HashMap<String, String>,
    pub query: std::collections::HashMap<String, String>,
    pub method: String,
    pub path: String,
    pub raw_query: Option<String>,
}
