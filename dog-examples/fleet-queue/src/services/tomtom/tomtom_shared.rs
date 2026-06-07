use crate::services::types::FleetParams;
use dog_core::{ServiceCapabilities, ServiceMethodKind};

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("geocode"),
        ServiceMethodKind::Custom("reverse-geocode"),
        ServiceMethodKind::Custom("search"),
        ServiceMethodKind::Custom("route"),
        ServiceMethodKind::Custom("eta"),
        ServiceMethodKind::Custom("traffic"),
        ServiceMethodKind::Custom("stats"),
    ])
}

pub fn register_hooks(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, FleetParams>,
) -> anyhow::Result<()> {
    Ok(())
}
