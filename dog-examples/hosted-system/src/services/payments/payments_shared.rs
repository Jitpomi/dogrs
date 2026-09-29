//! Service-local shared definitions and registration helpers for payments.

use anyhow::Result;
use dog_core::{DogAppBuilder, ServiceCapabilities, ServiceMethodKind};
use serde_json::Value;

/// Capabilities exposed by the payments service.
pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Create,
        ServiceMethodKind::Get,
        ServiceMethodKind::Remove,
    ])
}

/// Register schema and service hooks for the payments service.
pub fn register_hooks(app: &mut DogAppBuilder<Value, ()>) -> Result<()> {
    super::payments_schema::register(app)?;
    super::payments_hooks::register_hooks(app)?;
    Ok(())
}
