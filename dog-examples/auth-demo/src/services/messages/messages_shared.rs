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
    super::messages_schema::register(app)?;

    app.service("messages")?.hooks(|h| {
        // Protect all message operations with JWT authentication
        h.before_create(super::messages_hooks::authenticate("jwt"));
        h.before_find(super::messages_hooks::authenticate("jwt"));
        h.before_get(super::messages_hooks::authenticate("jwt"));
        h.before_update(super::messages_hooks::authenticate("jwt"));
        h.before_patch(super::messages_hooks::authenticate("jwt"));
        h.before_remove(super::messages_hooks::authenticate("jwt"));

        h.before_create(Arc::new(super::messages_hooks::ValidateMessageAuthorExists));
        h.before_patch(Arc::new(super::messages_hooks::ValidateMessageAuthorExists));

        h.after_find(Arc::new(super::messages_hooks::ExpandMessageAuthor));
        h.after(ServiceMethodKind::Get, Arc::new(super::messages_hooks::ExpandMessageAuthor));

        h.after_all(Arc::new(super::messages_hooks::NormalizeMessagesResult));
    });
    Ok(())
}
