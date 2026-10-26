//! # Application Construction (`app.rs`)
//!
//! Responsible for configuring the core [`DogAppBuilder`] instance:
//! - Applies configuration settings via [`crate::config::config`].
//! - Registers application-wide global hooks ([`crate::hooks`]) and broadcast channels ([`crate::channels`]).

use crate::services::CalendarParams;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

/// Builds and initializes the application builder with core configuration,
/// global hooks, and broadcast channels.
pub async fn build_builder() -> Result<DogAppBuilder<Value, CalendarParams>> {
    let mut builder: DogAppBuilder<Value, CalendarParams> = DogAppBuilder::new();

    crate::config::config(&mut builder)?;
    crate::hooks::register_global_hooks(&mut builder)?;
    crate::channels::configure(&mut builder)?;

    Ok(builder)
}
