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
    super::posts_schema::register(app)?;

    app.service("posts")?.hooks(|h| {
        h.after_all(Arc::new(super::posts_hooks::NormalizePostsResult));
    });
    Ok(())
}
