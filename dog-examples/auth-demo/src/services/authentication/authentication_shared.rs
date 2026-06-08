use std::sync::Arc;

use crate::services::AuthDemoParams;
pub fn register_hooks(
    builder: &mut dog_core::DogAppBuilder<serde_json::Value, AuthDemoParams>,
) -> anyhow::Result<()> {
    builder.service_hooks("authentication", |h| {
        // Protect write operations with JWT authentication_service_hooks::LogAuthCreate));
        h.after_create(Arc::new(
            super::authentication_service_hooks::StripPasswordFromAuthResult,
        ));
    });
    Ok(())
}
