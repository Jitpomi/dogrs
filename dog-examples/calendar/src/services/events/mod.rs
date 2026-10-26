//! # Events Service Module (`services/events/mod.rs`)
//!
//! Exposes the calendar events service implementation, schemas, hooks, and parameters.

pub mod event_params;
pub mod events_hooks;
pub mod events_schema;
pub mod events_service;
pub mod events_shared;

pub use event_params::EventParams;
pub use events_service::EventsService;
