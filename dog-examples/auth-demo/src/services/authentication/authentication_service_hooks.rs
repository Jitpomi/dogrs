use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogBeforeHook, HookContext, HookResult};
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

pub struct StripPasswordFromAuthResult;

#[async_trait]
impl DogAfterHook<Value, AuthDemoParams> for StripPasswordFromAuthResult {
    async fn run(&self, ctx: &mut HookContext<Value, AuthDemoParams>) -> Result<()> {
        let Some(result) = ctx.result.as_mut() else {
            return Ok(());
        };

        match result {
            HookResult::One(v) => {
                if let Some(user) = v.get_mut("user").and_then(|u| u.as_object_mut()) {
                    user.remove("password");
                }
            }
            HookResult::Many(items) => {
                for v in items {
                    if let Some(user) = v.get_mut("user").and_then(|u| u.as_object_mut()) {
                        user.remove("password");
                    }
                }
            }
        }

        Ok(())
    }
}
