use anyhow::Result;
use async_trait::async_trait;
use dog_core::tenant::TenantContext;
use dog_core::{DogService, ServiceCapabilities};
use serde_json::Value;

use crate::services::adapters::InMemoryAdapter;
use crate::services::AuthDemoParams;

use super::messages_shared;

pub struct MessagesService {
    pub adapter: InMemoryAdapter,
}

#[async_trait]
impl DogService<Value, AuthDemoParams> for MessagesService {
    fn capabilities(&self) -> ServiceCapabilities {
        messages_shared::crud_capabilities()
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
        self.adapter.find(ctx, params).await
    }

    async fn get(&self, ctx: &TenantContext, id: &str, params: AuthDemoParams) -> Result<Value> {
        self.adapter.get(ctx, id, params).await
    }

    async fn update(
        &self,
        ctx: &TenantContext,
        id: &str,
        data: Value,
        params: AuthDemoParams,
    ) -> Result<Value> {
        self.authorize_write(ctx, id, &params).await?;
        let mut data = data;
        data["sender"] = Value::String(super::messages_hooks::author(&params)?.to_owned());
        self.adapter.update(ctx, id, data, params).await
    }

    async fn patch(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        data: Value,
        params: AuthDemoParams,
    ) -> Result<Value> {
        self.authorize_write(
            ctx,
            id.ok_or_else(|| anyhow::anyhow!("message ID required"))?,
            &params,
        )
        .await?;
        self.adapter.patch(ctx, id, data, params).await
    }

    async fn remove(
        &self,
        ctx: &TenantContext,
        id: Option<&str>,
        params: AuthDemoParams,
    ) -> Result<Value> {
        self.authorize_write(
            ctx,
            id.ok_or_else(|| anyhow::anyhow!("message ID required"))?,
            &params,
        )
        .await?;
        self.adapter.remove(ctx, id, params).await
    }
}

impl MessagesService {
    async fn authorize_write(
        &self,
        ctx: &TenantContext,
        id: &str,
        params: &AuthDemoParams,
    ) -> Result<()> {
        let author = super::messages_hooks::author(params)?;
        let existing = self.adapter.get(ctx, id, params.clone()).await?;
        if existing.get("sender").and_then(Value::as_str) != Some(author) {
            return Err(dog_core::errors::DogError::forbidden(
                "Only the author can change a message",
            )
            .into());
        }
        Ok(())
    }
    pub fn new() -> Self {
        Self {
            adapter: InMemoryAdapter::new("message"),
        }
    }
}
