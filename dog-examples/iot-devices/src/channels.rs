use crate::services::DemoParams;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

pub fn configure(
    builder: &mut dog_core::DogAppBuilder<Value, DemoParams>,
) -> anyhow::Result<()> {
    let (tx, _) = broadcast::channel::<serde_json::Value>(100);
    builder.set("event_channel", Arc::new(tx.clone()));

    // Listen to all events on "*.*"
    builder.on_str("*.*", Arc::new(move |data, ctx| {
        let tx = tx.clone();
        let event_name = match &ctx.method {
            dog_core::ServiceMethodKind::Create => "created",
            dog_core::ServiceMethodKind::Update => "updated",
            dog_core::ServiceMethodKind::Patch => "patched",
            dog_core::ServiceMethodKind::Remove => "removed",
            dog_core::ServiceMethodKind::Custom(name) => *name,
            _ => "updated",
        }.to_string();

        let payload = match data {
            dog_core::events::ServiceEventData::Standard(res) => {
                match res {
                    dog_core::hooks::HookResult::One(val) => Some(val.clone()),
                    dog_core::hooks::HookResult::Many(vals) => vals.first().cloned(),
                }
            }
            dog_core::events::ServiceEventData::Custom(any_payload) => {
                any_payload.downcast_ref::<Value>().cloned()
            }
        };

        Box::pin(async move {
            if let Some(p) = payload {
                let json_payload = serde_json::json!({
                    "event": event_name,
                    "data": p
                });
                let _ = tx.send(json_payload);
            }
            Ok(())
        })
    }))?;

    Ok(())
}
