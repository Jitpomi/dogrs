use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast;
use serde_json::Value;
use crate::services::FleetParams;

/// Telemetry channel broadcaster singleton
pub fn telemetry_channel() -> &'static broadcast::Sender<Value> {
    static TELEMETRY_CHANNEL: OnceLock<broadcast::Sender<Value>> = OnceLock::new();
    TELEMETRY_CHANNEL.get_or_init(|| {
        let (tx, _) = broadcast::channel(100);
        tx
    })
}

/// Configures wildcard listener on DogApp to forward all service changes to the channel
pub fn configure(
    builder: &mut dog_core::DogAppBuilder<Value, FleetParams>,
) -> anyhow::Result<()> {
    let tx = telemetry_channel().clone();

    builder.on_str("*.*", Arc::new(move |data, _ctx| {
        let tx = tx.clone();
        Box::pin(async move {
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

            if let Some(p) = payload {
                let _ = tx.send(p);
            }
            Ok(())
        })
    }))?;

    Ok(())
}
