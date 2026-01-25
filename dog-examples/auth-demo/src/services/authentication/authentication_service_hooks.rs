use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogBeforeHook, HookContext};
use serde_json::Value;
use tracing::info;

use crate::services::AuthDemoParams;

pub struct LogAuthCreate;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for LogAuthCreate {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        let provider = ctx.params.provider.as_deref().unwrap_or("");
        let strategy = ctx
            .data
            .as_ref()
            .and_then(|v| v.get("strategy"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        info!(provider = provider, strategy = strategy, "auth.create");
        Ok(())
    }
}

pub struct LogAuthRemove;

#[async_trait]
impl DogBeforeHook<Value, AuthDemoParams> for LogAuthRemove {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        let provider = ctx.params.provider.as_deref().unwrap_or("");
        info!(provider = provider, "auth.remove");
        Ok(())
    }
}
