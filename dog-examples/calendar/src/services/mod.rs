//! # Application Services (`services/mod.rs`)
//!
//! Declares and registers application services into [`dog_core::DogAppBuilder`].
//! In this calendar example, the [`events::EventsService`] is registered and configured.

use std::sync::Arc;

use dog_core::DogService;

pub mod types;
pub use types::{CalendarParams, CalendarState};

pub mod adapters;

pub mod events;

/// Configures and registers application services and service-level hooks.
pub fn configure(
    app: &mut dog_core::DogAppBuilder<serde_json::Value, CalendarParams>,
    state: Arc<CalendarState>,
) -> anyhow::Result<()> {
    let events: Arc<dyn DogService<serde_json::Value, CalendarParams>> =
        Arc::new(events::EventsService::new(Arc::clone(&state)));
    app.register_service("events", Arc::clone(&events));
    events::events_shared::register_hooks(app)?;

    Ok(())
}
