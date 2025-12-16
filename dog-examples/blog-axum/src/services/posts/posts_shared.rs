use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

use dog_core::schema::SchemaHooksExt;

use crate::services::RelayParams;

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

pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, RelayParams>) -> anyhow::Result<()> {
    app.service("posts")?.hooks(|h| {
        h.schema(|s| {
            s.on_create()
                .resolve(super::posts_schema::resolve_create)
                .validate(super::posts_schema::validate_create);

            s.on_patch().validate(super::posts_schema::validate_patch);
            s.on_update().validate(super::posts_schema::validate_create);
        });

        h.after_all(Arc::new(super::posts_hooks::NormalizePostsResult));
    });
    Ok(())
}
