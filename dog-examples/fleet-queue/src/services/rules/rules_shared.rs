use crate::services::types::FleetParams;
use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

pub fn register_hooks(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, FleetParams>,
) -> anyhow::Result<()> {
    app.service_hooks("rules", |h| {
        h.before(
            dog_core::ServiceMethodKind::Find,
            Arc::new(super::rules_hooks::BeforeRead),
        );
        h.after(
            dog_core::ServiceMethodKind::Find,
            Arc::new(super::rules_hooks::AfterRead),
        );
        h.before(
            dog_core::ServiceMethodKind::Create,
            Arc::new(super::rules_hooks::BeforeWrite),
        );
        h.after(
            dog_core::ServiceMethodKind::Create,
            Arc::new(super::rules_hooks::AfterWrite),
        );
    });
    Ok(())
}
