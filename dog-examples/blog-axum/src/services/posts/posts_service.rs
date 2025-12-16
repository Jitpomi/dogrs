use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::{json, Value};

use crate::services::{RelayParams, RelayState};

use super::posts_shared;

pub struct PostsService {
    pub state: Arc<RelayState>,
}

#[async_trait]
impl DogService<Value, RelayParams> for PostsService {
    fn capabilities(&self) -> ServiceCapabilities {
        posts_shared::crud_capabilities()
    }

    async fn create(&self, _ctx: &TenantContext, data: Value, _params: RelayParams) -> Result<Value> {
        let _ = &self.state;
        Ok(data)
    }

    async fn find(&self, _ctx: &TenantContext, _params: RelayParams) -> Result<Vec<Value>> {
        Ok(vec![json!({})])
    }

    async fn get(&self, _ctx: &TenantContext, _id: &str, _params: RelayParams) -> Result<Value> {
        Ok(json!({ "id": _id }))
    }

    async fn update(&self, _ctx: &TenantContext, _id: &str, _data: Value, _params: RelayParams) -> Result<Value> {
        Ok(_data)
    }

    async fn patch(&self, _ctx: &TenantContext, _id: Option<&str>, _data: Value, _params: RelayParams) -> Result<Value> {
        let _ = _id;
        Ok(_data)
    }

    async fn remove(&self, _ctx: &TenantContext, _id: Option<&str>, _params: RelayParams) -> Result<Value> {
        Ok(json!({}))
    }
}
