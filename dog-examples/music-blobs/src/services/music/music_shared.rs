use crate::services::MusicParams;
use anyhow::Result;
use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Find,
        ServiceMethodKind::Remove,
        ServiceMethodKind::Custom("upload"),
        ServiceMethodKind::Custom("chunk"),
        ServiceMethodKind::Custom("complete"),
        ServiceMethodKind::Custom("stream"),
        ServiceMethodKind::Custom("pause"),
        ServiceMethodKind::Custom("resume"),
        ServiceMethodKind::Custom("cancel"),
        ServiceMethodKind::Custom("peaks"),
    ])
}

pub fn register_hooks(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, MusicParams>,
) -> Result<()> {
    app.service_hooks("music", |h| {
        h.before_all(Arc::new(super::music_hooks::ProcessMulterParams));
    });

    Ok(())
}
