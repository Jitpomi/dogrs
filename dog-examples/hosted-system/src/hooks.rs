//! Application-wide hooks and their registration.

use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

/// Register global hooks for the application.
pub fn register_global_hooks(_builder: &mut DogAppBuilder<Value, ()>) -> Result<()> {
    // Application-wide hooks are configured here.
    // The synthetic API authenticates with a dedicated bearer token at the HTTP transport boundary in app.rs.
    Ok(())
}
