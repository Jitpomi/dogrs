use anyhow::Result;
use dog_core::tenant::TenantContext;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::services::AuthDemoParams;

pub struct InMemoryAdapter {
    pub store: Arc<Mutex<HashMap<String, Value>>>,
    pub id_prefix: &'static str,
}

impl InMemoryAdapter {
    pub fn new(id_prefix: &'static str) -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
            id_prefix,
        }
    }

    pub async fn create(
        &self,
        _ctx: &TenantContext,
        data: Value,
        _params: AuthDemoParams,
    ) -> Result<Value> {
        let id = format!("{}_{}", self.id_prefix, Uuid::new_v4());
        let mut item = data;
        item["id"] = Value::String(id.clone());

        let mut store = self
            .store
            .lock()
            .map_err(|_| anyhow::anyhow!("demo storage lock poisoned"))?;
        anyhow::ensure!(store.len() < 10_000, "demo storage capacity reached");
        if self.id_prefix == "user" {
            for field in ["username", "googleId"] {
                if let Some(value) = item.get(field).and_then(Value::as_str) {
                    anyhow::ensure!(
                        !store
                            .values()
                            .any(|u| u.get(field).and_then(Value::as_str) == Some(value)),
                        "account identity already exists"
                    );
                }
            }
        }
        store.insert(id.clone(), item.clone());
        Ok(item)
    }

    pub async fn find(&self, _ctx: &TenantContext, _params: AuthDemoParams) -> Result<Vec<Value>> {
        let items = self
            .store
            .lock()
            .map_err(|_| anyhow::anyhow!("demo storage lock poisoned"))?
            .values()
            .cloned()
            .collect();
        Ok(items)
    }

    pub async fn get(
        &self,
        _ctx: &TenantContext,
        id: &str,
        _params: AuthDemoParams,
    ) -> Result<Value> {
        let item = self
            .store
            .lock()
            .map_err(|_| anyhow::anyhow!("demo storage lock poisoned"))?
            .get(id)
            .cloned();
        item.ok_or_else(|| anyhow::anyhow!("Item with id '{}' not found", id))
    }

    pub async fn update(
        &self,
        _ctx: &TenantContext,
        id: &str,
        mut data: Value,
        _params: AuthDemoParams,
    ) -> Result<Value> {
        data["id"] = Value::String(id.to_string());

        let mut store = self
            .store
            .lock()
            .map_err(|_| anyhow::anyhow!("demo storage lock poisoned"))?;
        if self.id_prefix == "user" {
            for field in ["username", "googleId"] {
                if let Some(value) = data.get(field).and_then(Value::as_str) {
                    anyhow::ensure!(
                        !store.iter().any(|(other_id, user)| other_id != id
                            && user.get(field).and_then(Value::as_str) == Some(value)),
                        "account identity already exists"
                    );
                }
            }
        }
        if store.contains_key(id) {
            store.insert(id.to_string(), data.clone());
            Ok(data)
        } else {
            Err(anyhow::anyhow!("Item with id '{}' not found", id))
        }
    }

    pub async fn patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        params: AuthDemoParams,
    ) -> Result<Value> {
        if let Some(id) = id {
            let mut existing = self.get(ctx, id, params.clone()).await?;

            // Merge the patch data into existing
            if let (Some(existing_obj), Some(patch_obj)) =
                (existing.as_object_mut(), data.as_object())
            {
                for (key, value) in patch_obj {
                    existing_obj.insert(key.clone(), value.clone());
                }
            }

            self.update(ctx, id, existing, params).await
        } else {
            Err(anyhow::anyhow!("ID is required for patch operation"))
        }
    }

    pub async fn remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        params: AuthDemoParams,
    ) -> Result<Value> {
        if let Some(id) = id {
            let item = self.get(ctx, id, params).await?;
            self.store
                .lock()
                .map_err(|_| anyhow::anyhow!("demo storage lock poisoned"))?
                .remove(id);
            Ok(item)
        } else {
            Err(anyhow::anyhow!("ID is required for remove operation"))
        }
    }
}
