use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogAroundHook, HookContext, Next};

use crate::services::MusicParams;

pub struct LogAround;

#[async_trait]
impl DogAroundHook<serde_json::Value, MusicParams> for LogAround {
    async fn run(
        &self,
        ctx: &mut HookContext<serde_json::Value, MusicParams>,
        next: Next<serde_json::Value, MusicParams>,
    ) -> Result<()> {
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
impl DogAfterHook<serde_json::Value, MusicParams> for LogAfter {
    async fn run(&self, ctx: &mut HookContext<serde_json::Value, MusicParams>) -> Result<()> {
        if let Some(err) = &ctx.error {
            eprintln!("[relay] <- ERROR: {err}");
        } else {
            eprintln!("[relay] <- OK");
        }

        Ok(())
    }
}

pub fn global_hooks(app: &dog_core::DogApp<serde_json::Value, MusicParams>) {
    app.hooks(|h| {
        h.around_all(Arc::new(LogAround));
        h.after_all(Arc::new(LogAfter));
    });
}
