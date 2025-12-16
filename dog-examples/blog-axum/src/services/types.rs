use std::collections::HashMap;

pub type BlogParams = dog_axum::params::RestParams;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct BlogState {
    pub posts_by_tenant: RwLock<HashMap<String, HashMap<String, serde_json::Value>>>,
}
