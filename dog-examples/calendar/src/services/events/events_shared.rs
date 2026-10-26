//! # Events Shared Definitions (`services/events/events_shared.rs`)
//!
//! Service capabilities declaration and hook registration helpers for the events service.

use dog_core::{ServiceCapabilities, ServiceMethodKind};
use std::sync::Arc;

use crate::services::CalendarParams;

/// Declares supported CRUD operations for the events service.
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

/// Registers declarative schemas and service hooks onto the [`dog_core::DogAppBuilder`].
pub fn register_hooks(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, CalendarParams>,
) -> anyhow::Result<()> {
    super::events_schema::register(app)?;

    app.service_hooks("events", |h| {
        h.after_all(Arc::new(super::events_hooks::NormalizeEventsResult));
    });
    Ok(())
}
