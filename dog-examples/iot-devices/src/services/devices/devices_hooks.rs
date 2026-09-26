use crate::services::DemoParams;
use dog_core::{DogAppBuilder, ServiceMethodKind, hooks::DogAfterHook};
use serde_json::Value;
use std::sync::Arc;

struct PublishCustomHook;

#[async_trait::async_trait]
impl DogAfterHook<Value, DemoParams> for PublishCustomHook {
    async fn run(&self, ctx: &mut dog_core::hooks::HookContext<Value, DemoParams>) -> anyhow::Result<()> {
        if let Some(res) = &ctx.result {
            let payload = match res {
                dog_core::hooks::HookResult::One(val) => val.clone(),
                dog_core::hooks::HookResult::Many(vals) => vals.first().cloned().unwrap_or(Value::Null),
            };
            if payload != Value::Null {
                let event_name = match &ctx.method {
                    ServiceMethodKind::Custom(name) => name.to_string(),
                    _ => "updated".to_string(),
                };
                ctx.services.app().emit_custom("devices", event_name, Arc::new(payload), ctx).await;
            }
        }
        Ok(())
    }
}

pub fn register_hooks(builder: &mut DogAppBuilder<Value, DemoParams>) -> anyhow::Result<()> {
    builder.service_hooks("devices", |hooks| {
        hooks.after(ServiceMethodKind::Custom("toggle"), Arc::new(PublishCustomHook));
        hooks.after(ServiceMethodKind::Custom("telemetry"), Arc::new(PublishCustomHook));
    });
    Ok(())
}
