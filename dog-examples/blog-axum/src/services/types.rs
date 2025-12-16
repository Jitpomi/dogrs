use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use dog_axum::params::{FromRestParams, RestParams};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct BlogParams(pub RestParams);

impl Deref for BlogParams {
    type Target = RestParams;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BlogParams {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromRestParams for BlogParams {
    fn from_rest_params(params: RestParams) -> Self {
        Self(params)
    }
}

#[derive(Default)]
pub struct BlogState {
    pub posts: RwLock<HashMap<String, serde_json::Value>>,
}
