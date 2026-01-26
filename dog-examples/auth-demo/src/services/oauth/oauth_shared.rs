use crate::services::AuthDemoParams;
use std::sync::Arc;
use dog_auth_local::hooks::ProtectHook;

pub fn crud_capabilities() -> dog_core::ServiceCapabilities {
    use dog_core::ServiceMethodKind;
    dog_core::ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("google_login"),
        ServiceMethodKind::Custom("google_callback"),
    ])
}

pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, AuthDemoParams>) -> anyhow::Result<()> {
    app.service("oauth")?.hooks(|_h| {
        _h.after_all(Arc::new(
            ProtectHook::from_deep_fields(&["password"])
                .with_paths(&["authentication.accessToken", "authentication.code"]),
        ));
    });
    Ok(())
}
