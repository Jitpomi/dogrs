use dog_core::{ServiceCapabilities, ServiceMethodKind};
use crate::services::DemoParams;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Find,
        ServiceMethodKind::Get,
        ServiceMethodKind::Create,
        ServiceMethodKind::Update,
        ServiceMethodKind::Patch,
        ServiceMethodKind::Remove,
        ServiceMethodKind::Custom("telemetry"),
        ServiceMethodKind::Custom("toggle"),
        ServiceMethodKind::Custom("stats"),
    ])
}

pub fn register_hooks(builder: &mut DogAppBuilder<Value, DemoParams>) -> anyhow::Result<()> {
    super::devices_hooks::register_hooks(builder)
}
