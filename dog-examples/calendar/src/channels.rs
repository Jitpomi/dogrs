//! # Broadcast Channels (`channels.rs`)
//!
//! Configures application-wide broadcast channels and event routing:
//! - Establishes a bounded Tokio broadcast channel (`event_channel`) to decouple
//!   in-memory service mutations from network consumers.
//! - Attaches a wildcard listener (`*.*`) to DogRS service lifecycle events,
//!   converting mutations (`created`, `updated`, `patched`, `removed`) into
//!   standardized JSON frames broadcast over WebSockets.

use crate::services::CalendarParams;
use dog_core::events::ServiceEventData;
use dog_core::hooks::HookResult;
use dog_core::ServiceMethodKind;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Configures the real-time event broadcasting channel and attaches lifecycle listeners.
pub fn configure(
    builder: &mut dog_core::DogAppBuilder<Value, CalendarParams>,
) -> anyhow::Result<()> {
    // 1. Create a bounded broadcast channel (128 event buffer).
    // Bounded buffers protect memory against slow WebSocket consumers.
    let (tx, _) = broadcast::channel::<serde_json::Value>(128);

    // 2. Store the sender in the app under "event_channel".
    // dog-transport's declare_ws_adapter automatically subscribes to this key.
    builder.set("event_channel", Arc::new(tx.clone()));

    // 3. Listen to all mutations across all services ("*.*")
    builder.on_str(
        "*.*",
        Arc::new(move |data, ctx| {
            let tx = tx.clone();
            let event_name = match &ctx.method {
                ServiceMethodKind::Create => "created",
                ServiceMethodKind::Update => "updated",
                ServiceMethodKind::Patch => "patched",
                ServiceMethodKind::Remove => "removed",
                ServiceMethodKind::Custom(name) => *name,
                _ => "updated",
            }
            .to_string();

            let payload = match data {
                ServiceEventData::Standard(res) => match res {
                    HookResult::One(val) => Some(val.clone()),
                    HookResult::Many(vals) => vals.first().cloned(),
                },
                ServiceEventData::Custom(any_payload) => {
                    any_payload.downcast_ref::<Value>().cloned()
                }
            };

            let tenant_id = ctx.tenant.tenant_id.0.clone();
            Box::pin(async move {
                if let Some(mut p) = payload {
                    if let Some(map) = p.as_object_mut() {
                        map.entry("tenant".to_string())
                            .or_insert_with(|| Value::String(tenant_id.clone()));
                    }
                    let json_payload = serde_json::json!({
                        "tenant": tenant_id,
                        "event": event_name,
                        "data": p
                    });
                    let _ = tx.send(json_payload);
                }
                Ok(())
            })
        }),
    )?;

    Ok(())
}
