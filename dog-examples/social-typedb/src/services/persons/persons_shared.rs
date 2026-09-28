use crate::services::SocialParams;
use anyhow::Result;
use dog_core::{ServiceCapabilities, ServiceMethodKind};

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

pub fn register_hooks(
    _app: &mut dog_core::DogAppBuilder<serde_json::Value, SocialParams>,
) -> Result<()> {
    // This loopback raw-query playground intentionally has no domain authorization hooks.
    Ok(())
}
