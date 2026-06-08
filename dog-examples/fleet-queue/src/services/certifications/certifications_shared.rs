use crate::services::types::FleetParams;
use dog_core::{ServiceCapabilities, ServiceMethodKind};

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

pub fn register_hooks(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, FleetParams>,
) -> anyhow::Result<()> {
    Ok(())
}
