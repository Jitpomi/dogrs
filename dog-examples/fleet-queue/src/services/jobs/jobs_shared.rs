use crate::services::FleetParams;
use dog_core::{DogAppBuilder, ServiceCapabilities, ServiceMethodKind};
use serde_json::Value;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("enqueue"),
        ServiceMethodKind::Custom("stats"),
        ServiceMethodKind::Custom("queue_status"),
    ])
}

pub fn register_hooks(app: &mut DogAppBuilder<Value, FleetParams>) -> anyhow::Result<()> {
    app.service_hooks("jobs", |h| {
        h.before(
            ServiceMethodKind::Custom("enqueue"),
            std::sync::Arc::new(super::jobs_hooks::BeforeEnqueue),
        );
        h.after(
            ServiceMethodKind::Custom("enqueue"),
            std::sync::Arc::new(super::jobs_hooks::AfterEnqueue),
        );
        h.before(
            ServiceMethodKind::Custom("stats"),
            std::sync::Arc::new(super::jobs_hooks::BeforeStats),
        );
        h.after(
            ServiceMethodKind::Custom("stats"),
            std::sync::Arc::new(super::jobs_hooks::AfterStats),
        );
    });

    Ok(())
}
