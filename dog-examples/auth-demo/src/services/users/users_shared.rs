use dog_core::{ServiceCapabilities, ServiceMethodKind};
use anyhow::anyhow;
use std::sync::Arc;

use crate::services::AuthDemoParams;
use dog_auth::hooks::AuthenticateHook;
use dog_core::hooks::DogBeforeHook;
use dog_auth_local::hooks::{HashPasswordHook, ProtectHook};
use dog_auth_local::LocalStrategy;
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

pub fn register_hooks(
    builder: &mut dog_core::DogAppBuilder<serde_json::Value, AuthDemoParams>,
    auth_core: Arc<dog_auth::AuthenticationService<AuthDemoParams>>,
) -> anyhow::Result<()> {
    super::users_schema::register(builder)?;

    let local = builder
        .get::<Arc<LocalStrategy<AuthDemoParams>>>("auth.local")
        .ok_or_else(|| anyhow!("Missing auth.local in app config"))?;

    let jwt: Arc<dyn DogBeforeHook<Value, AuthDemoParams>> =
        Arc::new(AuthenticateHook::new(auth_core, vec!["jwt".to_string()]));

    builder.service_hooks("users", |h| {
        h.before_create(Arc::new(HashPasswordHook::new("password", Arc::clone(&local))));
        // Protect everything except create/find
        h.before_get(Arc::clone(&jwt));
        h.before_update(Arc::clone(&jwt));
        h.before_patch(Arc::clone(&jwt));
        h.before_remove(Arc::clone(&jwt));

        // Ensure password is hashed for writes
        h.before_update(Arc::new(HashPasswordHook::new("password", Arc::clone(&local))));
        h.before_patch(Arc::new(HashPasswordHook::new("password", Arc::clone(&local))));
        h.after_all(Arc::new(ProtectHook::from_fields(&["password"])));
        h.before_remove(Arc::new(super::users_hooks::EnforceUserOnDelete));
    });

    Ok(())
}
