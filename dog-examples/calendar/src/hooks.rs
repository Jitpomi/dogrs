//! # Global Application Hooks (`hooks.rs`)
//!
//! Defines and registers application-wide lifecycle hooks:
//! - [`LogAround`]: An around-hook that logs incoming service method execution.
//! - [`LogAfter`]: An after-hook that logs execution completion status and captures errors.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogAroundHook, HookContext, Next};

use crate::services::CalendarParams;

/// Around-hook logging incoming request details before delegating to the next handler.
pub struct LogAround;

#[async_trait]
impl DogAroundHook<serde_json::Value, CalendarParams> for LogAround {
    async fn run(
        &self,
        ctx: &mut HookContext<serde_json::Value, CalendarParams>,
        next: Next<serde_json::Value, CalendarParams>,
    ) -> Result<()> {
        let provider = ctx.params.provider.clone();
        let path = ctx.params.path.clone();
        let method = ctx.params.method.clone();

        tracing::debug!("[relay] -> {method} {path} provider={provider}");

        next.run(ctx).await?;

        Ok(())
    }
}

/// After-hook logging the operation outcome or error status.
pub struct LogAfter;

#[async_trait]
impl DogAfterHook<serde_json::Value, CalendarParams> for LogAfter {
    async fn run(&self, ctx: &mut HookContext<serde_json::Value, CalendarParams>) -> Result<()> {
        if let Some(err) = &ctx.error {
            tracing::warn!("[relay] <- ERROR: {err}");
        } else {
            tracing::debug!("[relay] <- OK");
        }

        Ok(())
    }
}

/// Registers application-wide global hooks onto the [`dog_core::DogAppBuilder`].
pub fn register_global_hooks(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, CalendarParams>,
) -> anyhow::Result<()> {
    app.hooks(|h| {
        h.around_all(Arc::new(LogAround));
        h.after_all(Arc::new(LogAfter));
    });
    Ok(())
}
