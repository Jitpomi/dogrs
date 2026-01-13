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
    app.service("employees")?.hooks(|h| {
        h.before_find(Arc::new(super::employees_hooks::BeforeRead));
        h.after_find(Arc::new(super::employees_hooks::AfterRead));
        h.before_create(Arc::new(super::employees_hooks::BeforeWrite));
        h.after_create(Arc::new(super::employees_hooks::AfterWrite));
    });
    
    Ok(())
}
