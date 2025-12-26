use anyhow::Result;
use dog_core::DogApp;
use dog_core::{ServiceCapabilities, ServiceMethodKind};
use crate::services::SocialParams;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

pub fn register_hooks(_app: &DogApp<serde_json::Value, SocialParams>) -> Result<()> {
    // TODO: Implement persons hooks for TypeDB social network
    // - Validate person profile data and privacy settings
    // - Handle friendship and following relationships
    // - Manage birth, employment, and education relationships
    Ok(())
}
