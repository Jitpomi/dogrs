use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::DogError;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;
use uuid::Uuid;

use crate::services::{BlogParams, BlogState};

use super::posts_shared;
use super::PostParams;

pub struct PostsService {
    pub state: Arc<BlogState>,
}

#[async_trait]
impl DogService<Value, BlogParams> for PostsService {
    fn capabilities(&self) -> ServiceCapabilities {
        posts_shared::crud_capabilities()
    }

    async fn create(&self, _ctx: &TenantContext, data: Value, _params: BlogParams) -> Result<Value> {
        let mut obj = data.as_object().cloned().unwrap_or_default();

        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("post:{}", Uuid::new_v4()));

        obj.insert("id".to_string(), Value::String(id.clone()));

        let value = Value::Object(obj);
        self.state.posts.write().await.insert(id, value.clone());
        Ok(value)
    }

    async fn find(&self, _ctx: &TenantContext, _params: BlogParams) -> Result<Vec<Value>> {
        let post_params = PostParams::from(&_params);
        let map = self.state.posts.read().await;
        Ok(map
            .values()
            .cloned()
            .filter(|v| {
                post_params.include_drafts
                    || v.get("published").and_then(|v| v.as_bool()).unwrap_or(false)
            })
            .collect())
    }

    async fn get(&self, _ctx: &TenantContext, _id: &str, _params: BlogParams) -> Result<Value> {
        let map = self.state.posts.read().await;
        map.get(_id)
            .cloned()
            .ok_or_else(|| DogError::not_found(format!("Post not found: {_id}")).into_anyhow())
    }

    async fn update(&self, _ctx: &TenantContext, _id: &str, _data: Value, _params: BlogParams) -> Result<Value> {
        let mut map = self.state.posts.write().await;
        if !map.contains_key(_id) {
            return Err(DogError::not_found(format!("Post not found: {_id}")).into_anyhow());
        }

        let mut obj = _data.as_object().cloned().unwrap_or_default();
        obj.insert("id".to_string(), Value::String(_id.to_string()));
        let value = Value::Object(obj);
        map.insert(_id.to_string(), value.clone());
        Ok(value)
    }

    async fn patch(&self, _ctx: &TenantContext, _id: Option<&str>, _data: Value, _params: BlogParams) -> Result<Value> {
        let Some(id) = _id else {
            return Err(DogError::bad_request("Patch requires an id").into_anyhow());
        };

        let mut map = self.state.posts.write().await;
        let existing = map
            .get(id)
            .cloned()
            .ok_or_else(|| DogError::not_found(format!("Post not found: {id}")).into_anyhow())?;

        let mut base = existing.as_object().cloned().unwrap_or_default();
        if let Some(patch) = _data.as_object() {
            for (k, v) in patch {
                if k == "id" {
                    continue;
                }
                base.insert(k.clone(), v.clone());
            }
        }

        base.insert("id".to_string(), Value::String(id.to_string()));
        let value = Value::Object(base);
        map.insert(id.to_string(), value.clone());
        Ok(value)
    }

    async fn remove(&self, _ctx: &TenantContext, _id: Option<&str>, _params: BlogParams) -> Result<Value> {
        let Some(id) = _id else {
            return Err(DogError::bad_request("Remove requires an id").into_anyhow());
        };

        let mut map = self.state.posts.write().await;
        map.remove(id)
            .ok_or_else(|| DogError::not_found(format!("Post not found: {id}")).into_anyhow())
    }
}
