use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogAroundHook, HookContext, Next};

use crate::services::BlogParams;

pub struct LogAround;

#[async_trait]
impl DogAroundHook<serde_json::Value, BlogParams> for LogAround {
    async fn run(&self, ctx: &mut HookContext<serde_json::Value, BlogParams>, next: Next<serde_json::Value, BlogParams>) -> Result<()> {
        let provider = ctx.params.provider.clone();
        let path = ctx.params.path.clone();
        let method = ctx.params.method.clone();

        eprintln!("[relay] -> {method} {path} provider={provider}");

        next.run(ctx).await?;

        Ok(())
    }
}

pub struct LogAfter;

#[async_trait]
impl DogAfterHook<serde_json::Value, BlogParams> for LogAfter {
    async fn run(&self, ctx: &mut HookContext<serde_json::Value, BlogParams>) -> Result<()> {
        if let Some(err) = &ctx.error {
            eprintln!("[relay] <- ERROR: {err}");
        } else {
            eprintln!("[relay] <- OK");
        }

        Ok(())
    }
}

pub fn global_hooks(app: &dog_core::DogApp<serde_json::Value, BlogParams>) {
    app.hooks(|h| {
        h.around_all(Arc::new(LogAround));
        h.after_all(Arc::new(LogAfter));
    });
}
