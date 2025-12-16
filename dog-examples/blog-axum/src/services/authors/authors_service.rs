use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::errors::DogError;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;
use uuid::Uuid;

use crate::services::{BlogParams, BlogState};

use super::authors_shared;

pub struct AuthorsService {
    pub state: Arc<BlogState>,
}

#[async_trait]
impl DogService<Value, BlogParams> for AuthorsService {
    fn capabilities(&self) -> ServiceCapabilities {
        authors_shared::crud_capabilities()
    }

    async fn create(&self, ctx: &TenantContext, data: Value, _params: BlogParams) -> Result<Value> {
        let mut obj = data.as_object().cloned().unwrap_or_default();

        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("author:{}", Uuid::new_v4()));

        obj.insert("id".to_string(), Value::String(id.clone()));

        let value = Value::Object(obj);

        let tenant = ctx.tenant_id.0.clone();
        let mut by_tenant = self.state.authors_by_tenant.write().await;
        by_tenant.entry(tenant).or_default().insert(id, value.clone());

        Ok(value)
    }

    async fn find(&self, ctx: &TenantContext, _params: BlogParams) -> Result<Vec<Value>> {
        let tenant = ctx.tenant_id.0.clone();
        let by_tenant = self.state.authors_by_tenant.read().await;
        let map = by_tenant.get(&tenant);
        Ok(map.into_iter().flat_map(|m| m.values()).cloned().collect())
    }

    async fn get(&self, ctx: &TenantContext, id: &str, _params: BlogParams) -> Result<Value> {
        let tenant = ctx.tenant_id.0.clone();
        let by_tenant = self.state.authors_by_tenant.read().await;
        let map = by_tenant.get(&tenant);
        map.and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(|| DogError::not_found(format!("Author not found: {id}")).into_anyhow())
    }

    async fn update(&self, ctx: &TenantContext, id: &str, data: Value, _params: BlogParams) -> Result<Value> {
        let tenant = ctx.tenant_id.0.clone();
        let mut by_tenant = self.state.authors_by_tenant.write().await;
        let map = by_tenant.entry(tenant).or_default();
        if !map.contains_key(id) {
            return Err(DogError::not_found(format!("Author not found: {id}")).into_anyhow());
        }

        let mut obj = data.as_object().cloned().unwrap_or_default();
        obj.insert("id".to_string(), Value::String(id.to_string()));
        let value = Value::Object(obj);
        map.insert(id.to_string(), value.clone());
        Ok(value)
    }

    async fn patch(&self, ctx: &TenantContext, id: Option<&str>, data: Value, _params: BlogParams) -> Result<Value> {
        let Some(id) = id else {
            return Err(DogError::bad_request("Patch requires an id").into_anyhow());
        };

        let tenant = ctx.tenant_id.0.clone();
        let mut by_tenant = self.state.authors_by_tenant.write().await;
        let map = by_tenant.entry(tenant).or_default();

        let existing = map
            .get(id)
            .cloned()
            .ok_or_else(|| DogError::not_found(format!("Author not found: {id}")).into_anyhow())?;

        let mut base = existing.as_object().cloned().unwrap_or_default();
        if let Some(patch) = data.as_object() {
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

    async fn remove(&self, ctx: &TenantContext, id: Option<&str>, _params: BlogParams) -> Result<Value> {
        let Some(id) = id else {
            return Err(DogError::bad_request("Remove requires an id").into_anyhow());
        };

        let tenant = ctx.tenant_id.0.clone();
        let mut by_tenant = self.state.authors_by_tenant.write().await;
        let map = by_tenant.entry(tenant).or_default();
        map.remove(id)
            .ok_or_else(|| DogError::not_found(format!("Author not found: {id}")).into_anyhow())
    }
}
