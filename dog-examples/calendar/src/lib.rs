//! # Real-Time Calendar Library (`lib.rs`)
//!
//! Exposes module declarations and the [`build`] constructor function for
//! embedding the calendar application in servers, integration tests, or custom hosts.

mod app;
pub mod background;
mod channels;
pub mod config;
mod hooks;
pub mod services;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::Result;
use dog_core::DogApp;
use dog_transport::{http::DogHttpService, HttpOptions, IntoDogService};
use serde_json::Value;

pub use crate::services::CalendarParams;

/// Assembles the complete DogRS application:
/// 1. Initializes the application builder, global hooks, and broadcast channels.
/// 2. Initializes the `dog-queue` memory backend and starts reminder workers.
/// 3. Configures services and multi-tenant in-memory state.
/// 4. Returns the built [`DogApp`] and [`DogHttpService`] bound to `/events`.
pub async fn build() -> Result<(
    DogApp<Value, CalendarParams>,
    DogHttpService<Value, CalendarParams>,
)> {
    let mut builder = app::build_builder().await?;

    // Initialize dog-queue for event reminder scheduling
    let queue = Arc::new(background::CalendarQueue::new().await?);

    if let Some(event_channel) =
        builder.get::<Arc<tokio::sync::broadcast::Sender<Value>>>("event_channel")
    {
        queue
            .start_workers(background::CalendarJobContext {
                event_channel: Arc::clone(&event_channel),
            })
            .await?;
    }

    let state = Arc::new(services::CalendarState {
        events_by_tenant: RwLock::new(HashMap::new()),
        reminder_jobs: RwLock::new(HashMap::new()),
        queue,
    });

    services::configure(&mut builder, Arc::clone(&state))?;

    let dog = builder.build();

    let http_service = dog
        .clone()
        .into_service(HttpOptions::default().route("/events", "events"));

    Ok((dog, http_service))
}
