use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;
use crate::services::types::FleetParams;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, FleetParams>) -> anyhow::Result<()> {
    app.service("vehicles")?.hooks(|h| {
        h.before_find(Arc::new(super::vehicles_hooks::BeforeRead));
        h.after_find(Arc::new(super::vehicles_hooks::AfterRead));
        h.before_create(Arc::new(super::vehicles_hooks::BeforeWrite));
        h.after_create(Arc::new(super::vehicles_hooks::AfterWrite));
    });
    
    Ok(())
}
