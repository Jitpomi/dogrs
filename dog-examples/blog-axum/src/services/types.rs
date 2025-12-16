pub type RelayParams = dog_axum::params::RestParams;

use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct BlogState {
    pub posts: RwLock<HashMap<String, serde_json::Value>>,
}
