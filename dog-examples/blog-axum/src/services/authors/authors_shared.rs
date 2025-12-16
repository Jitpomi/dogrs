use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

use crate::services::BlogParams;

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

pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, BlogParams>) -> anyhow::Result<()> {
    super::authors_schema::register(app)?;

    app.service("authors")?.hooks(|h| {
        h.before_remove(Arc::new(super::authors_hooks::EnforceAuthorOnDelete));
    });

    Ok(())
}
