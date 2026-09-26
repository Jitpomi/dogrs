use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dog_core::hooks::{DogAfterHook, DogAroundHook, HookContext, Next};

use crate::services::FleetParams;

pub struct LogAround;

#[async_trait]
impl DogAroundHook<serde_json::Value, FleetParams> for LogAround {
    async fn run(
        &self,
        ctx: &mut HookContext<serde_json::Value, FleetParams>,
        next: Next<serde_json::Value, FleetParams>,
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
impl DogAfterHook<serde_json::Value, FleetParams> for LogAfter {
    async fn run(&self, ctx: &mut HookContext<serde_json::Value, FleetParams>) -> Result<()> {
        if let Some(err) = &ctx.error {
            eprintln!("[relay] <- ERROR: {err}");
        } else {
            eprintln!("[relay] <- OK");

            // Automatically emit custom events for raw write queries so they propagate via SSE
            if matches!(ctx.method, dog_core::ServiceMethodKind::Custom(ref m) if *m == "write") {
                let mut service_name = ctx.params.path.trim_matches('/').split('/').next().unwrap_or("");
                if service_name.is_empty() {
                    service_name = "operations";
                }
                if !service_name.is_empty() {
                    if let Some(res) = &ctx.result {
                        match res {
                            dog_core::hooks::HookResult::One(val) => {
                                let _ = ctx.app().emit_custom(service_name, "write", Arc::new(val.clone()), ctx).await;
                            }
                            dog_core::hooks::HookResult::Many(vals) => {
                                if let Some(first) = vals.first() {
                                    let _ = ctx.app().emit_custom(service_name, "write", Arc::new(first.clone()), ctx).await;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn global_hooks(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, FleetParams>,
) -> Result<()> {
    app.hooks(|h| {
        h.around_all(Arc::new(LogAround));
        h.after_all(Arc::new(LogAfter));
    });
    Ok(())
}
