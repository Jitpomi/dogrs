use dog_core::{ServiceCapabilities, ServiceMethodKind, DogApp};
use serde_json::Value;
use crate::services::FleetParams;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("enqueue"),
        ServiceMethodKind::Custom("stats"),
        ServiceMethodKind::Custom("queue_status"),
    ])
}

pub fn register_hooks(app: &DogApp<Value, FleetParams>) -> anyhow::Result<()> {
    app.service("jobs")?.hooks(|h| {
        h.before(ServiceMethodKind::Custom("enqueue"), std::sync::Arc::new(super::jobs_hooks::BeforeEnqueue));
        h.after(ServiceMethodKind::Custom("enqueue"), std::sync::Arc::new(super::jobs_hooks::AfterEnqueue));
        h.before(ServiceMethodKind::Custom("stats"), std::sync::Arc::new(super::jobs_hooks::BeforeStats));
        h.after(ServiceMethodKind::Custom("stats"), std::sync::Arc::new(super::jobs_hooks::AfterStats));
    });
    
    Ok(())
}
