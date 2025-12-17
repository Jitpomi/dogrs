use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use dog_core::errors::DogError;
use dog_core::tenant::TenantContext;
use dog_core::ServiceCapabilities;
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::services::{BlogParams, BlogState};

#[derive(Clone, Copy)]
pub enum StoreKind {
    Posts,
    Authors,
}

pub struct BlogAdapter {
    pub state: Arc<BlogState>,
    pub store: StoreKind,
    pub id_prefix: &'static str,
    pub not_found_prefix: &'static str,
    pub capabilities: ServiceCapabilities,
}

impl BlogAdapter {
    fn map_for(&self) -> &RwLock<HashMap<String, HashMap<String, Value>>> {
        match self.store {
            StoreKind::Posts => &self.state.posts_by_tenant,
            StoreKind::Authors => &self.state.authors_by_tenant,
        }
    }

    fn tenant_key(ctx: &TenantContext) -> String {
        ctx.tenant_id.0.clone()
    }

    fn not_found(&self, id: &str) -> anyhow::Error {
        DogError::not_found(format!("{}: {id}", self.not_found_prefix)).into_anyhow()
    }

    fn require_id<'a>(&self, id: Option<&'a str>, msg: &'static str) -> Result<&'a str> {
        id.ok_or_else(|| DogError::bad_request(msg).into_anyhow())
    }

    pub async fn _create(&self, ctx: &TenantContext, data: Value, _params: BlogParams) -> Result<Value> {
        let mut obj = data.as_object().cloned().unwrap_or_default();

        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}:{}", self.id_prefix, Uuid::new_v4()));

        obj.insert("id".to_string(), Value::String(id.clone()));
        let value = Value::Object(obj);

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.map_for().write().await;
        by_tenant.entry(tenant).or_default().insert(id, value.clone());

        Ok(value)
    }

    pub async fn _find(&self, ctx: &TenantContext, _params: BlogParams) -> Result<Vec<Value>> {
        let tenant = Self::tenant_key(ctx);
        let by_tenant = self.map_for().read().await;
        let map = by_tenant.get(&tenant);
        Ok(map.into_iter().flat_map(|m| m.values()).cloned().collect())
    }

    pub async fn _get(&self, ctx: &TenantContext, id: &str, _params: BlogParams) -> Result<Value> {
        let tenant = Self::tenant_key(ctx);
        let by_tenant = self.map_for().read().await;
        let map = by_tenant.get(&tenant);
        map.and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(|| self.not_found(id))
    }

    pub async fn _update(&self, ctx: &TenantContext, id: &str, data: Value, _params: BlogParams) -> Result<Value> {
        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.map_for().write().await;
        let map = by_tenant.entry(tenant).or_default();
        if !map.contains_key(id) {
            return Err(self.not_found(id));
        }

        let mut obj = data.as_object().cloned().unwrap_or_default();
        obj.insert("id".to_string(), Value::String(id.to_string()));
        let value = Value::Object(obj);
        map.insert(id.to_string(), value.clone());
        Ok(value)
    }

    pub async fn _patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        _params: BlogParams,
    ) -> Result<Value> {
        let id = self.require_id(id, "Patch requires an id")?;

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.map_for().write().await;
        let map = by_tenant.entry(tenant).or_default();

        let existing = map
            .get(id)
            .cloned()
            .ok_or_else(|| self.not_found(id))?;

        let mut record = existing.as_object().cloned().unwrap_or_default();
        if let Some(patch) = data.as_object() {
            for (k, v) in patch {
                if k == "id" {
                    continue;
                }
                record.insert(k.clone(), v.clone());
            }
        }

        record.insert("id".to_string(), Value::String(id.to_string()));
        let value = Value::Object(record);
        map.insert(id.to_string(), value.clone());
        Ok(value)
    }

    pub async fn _remove(&self, ctx: &TenantContext, id: Option<&str>, _params: BlogParams) -> Result<Value> {
        let id = self.require_id(id, "Remove requires an id")?;

        let tenant = Self::tenant_key(ctx);
        let mut by_tenant = self.map_for().write().await;
        let map = by_tenant.entry(tenant).or_default();
        map.remove(id)
            .ok_or_else(|| self.not_found(id))
    }
}

dog_core::dog_adapter!(
    BlogAdapter,
    serde_json::Value,
    crate::services::BlogParams
);
