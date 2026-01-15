use dog_core::{ServiceCapabilities, ServiceMethodKind};
use crate::services::types::FleetParams;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("geocode"),
        ServiceMethodKind::Custom("search"),
        ServiceMethodKind::Custom("route"),
        ServiceMethodKind::Custom("eta"),
        ServiceMethodKind::Custom("traffic"),
        ServiceMethodKind::Custom("stats"),
    ])
}

pub fn register_hooks(_app: &dog_core::DogApp<serde_json::Value, FleetParams>) -> anyhow::Result<()> {
    // TomTom service doesn't use traditional hooks since it's queue-based
    // Hooks are registered on other services (deliveries, vehicles) that trigger TomTom operations
    Ok(())
}
