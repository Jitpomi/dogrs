use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;

use crate::services::adapters::InMemoryAdapter;
use crate::services::AuthDemoParams;

use super::users_shared;

pub struct UsersService {
    pub adapter: InMemoryAdapter,
}

#[async_trait]
impl DogService<Value, AuthDemoParams> for UsersService {
    fn capabilities(&self) -> ServiceCapabilities {
        users_shared::crud_capabilities()
    }

    async fn create(
        &self,
        ctx: &TenantContext,
        data: Value,
        params: AuthDemoParams,
    ) -> Result<Value> {
        self.adapter.create(ctx, data, params).await
    }

    async fn find(&self, ctx: &TenantContext, params: AuthDemoParams) -> Result<Vec<Value>> {
        if params.authenticated {
            let id = params
                .auth_result
                .as_ref()
                .and_then(|r| r.get("user"))
                .and_then(|u| u.get("id"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing authenticated user"))?
                .to_owned();
            return Ok(vec![self.adapter.get(ctx, &id, params).await?]);
        }
        self.adapter.find(ctx, params).await
    }

    async fn get(&self, ctx: &TenantContext, id: &str, params: AuthDemoParams) -> Result<Value> {
        super::users_hooks::authorize_user(&params, id)?;
        self.adapter.get(ctx, id, params).await
    }

    async fn update(
        &self,
        ctx: &TenantContext,
        id: &str,
        data: Value,
        params: AuthDemoParams,
    ) -> Result<Value> {
        super::users_hooks::authorize_user(&params, id)?;
        self.adapter.update(ctx, id, data, params).await
    }

    async fn patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        params: AuthDemoParams,
    ) -> Result<Value> {
        super::users_hooks::authorize_user(
            &params,
            id.ok_or_else(|| anyhow::anyhow!("user ID required"))?,
        )?;
        self.adapter.patch(ctx, id, data, params).await
    }

    async fn remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        params: AuthDemoParams,
    ) -> Result<Value> {
        super::users_hooks::authorize_user(
            &params,
            id.ok_or_else(|| anyhow::anyhow!("user ID required"))?,
        )?;
        self.adapter.remove(ctx, id, params).await
    }
}

impl UsersService {
    pub fn new() -> Self {
        Self {
            adapter: InMemoryAdapter::new("user"),
        }
    }
}
