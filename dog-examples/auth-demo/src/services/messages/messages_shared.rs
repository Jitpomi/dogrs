use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

use crate::services::AuthDemoParams;
use dog_auth::hooks::AuthenticateHook;
use dog_core::hooks::DogBeforeHook;
use serde_json::Value;

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

    let jwt: Arc<dyn DogBeforeHook<Value, AuthDemoParams>> =
        Arc::new(AuthenticateHook::from_app(app, vec!["jwt".to_string()])?);

    app.service("messages")?.hooks(|h| {
        // Protect write operations with JWT authentication
        h.before_create(Arc::clone(&jwt));
        h.before_patch(Arc::clone(&jwt));
        h.before_remove(Arc::clone(&jwt));

        h.before_create(Arc::new(super::messages_hooks::ValidateMessageAuthorExists));
        h.before_patch(Arc::new(super::messages_hooks::ValidateMessageAuthorExists));

        h.after_find(Arc::new(super::messages_hooks::ExpandMessageAuthor));
        h.after(ServiceMethodKind::Get, Arc::new(super::messages_hooks::ExpandMessageAuthor));

        h.after_all(Arc::new(super::messages_hooks::NormalizeMessagesResult));
    });
    Ok(())
}
