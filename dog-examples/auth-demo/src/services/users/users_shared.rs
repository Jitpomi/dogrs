use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

use crate::services::AuthDemoParams;

pub fn crud_capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Create,
        ServiceMethodKind::Find,
        ServiceMethodKind::Get,
        ServiceMethodKind::Update,
        ServiceMethodKind::Patch,
        ServiceMethodKind::Remove,
    ])
}

pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, AuthDemoParams>) -> anyhow::Result<()> {
    super::users_schema::register(app)?;

    app.service("users")?.hooks(|h| {
        h.before_remove(Arc::new(super::users_hooks::EnforceUserOnDelete));
    });

    Ok(())
}
