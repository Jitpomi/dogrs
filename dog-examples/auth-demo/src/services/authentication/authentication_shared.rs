use std::sync::Arc;

use crate::services::AuthDemoParams;
pub fn register_hooks(app: &dog_core::DogApp<serde_json::Value, AuthDemoParams>) -> anyhow::Result<()> {
    app.service("authentication")?.hooks(|h| {
        h.before_create(Arc::new(super::authentication_service_hooks::LogAuthCreate));
        h.after_create(Arc::new(super::authentication_service_hooks::StripPasswordFromAuthResult));
        h.before_remove(Arc::new(super::authentication_service_hooks::LogAuthRemove));
    });
    Ok(())
}
